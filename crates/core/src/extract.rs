use crate::{ContentBlock, Event, EventType, Session};

/// Metadata extracted from a session for DB storage at upload time.
#[derive(Debug, Clone)]
pub struct UploadMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub created_at: String,
    pub working_directory: Option<String>,
    pub files_modified: Option<String>,
    pub files_read: Option<String>,
    pub has_errors: bool,
}

/// Extract upload metadata from a session, auto-generating title/description
/// from the first user messages when the session's own metadata is empty.
///
/// This consolidates the duplicated logic in server and worker upload handlers.
pub fn extract_upload_metadata(session: &Session) -> UploadMetadata {
    let title = session
        .context
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| extract_first_user_text(session).map(|t| truncate_str(&t, 80)));

    let description = session
        .context
        .description
        .clone()
        .filter(|d| !d.is_empty())
        .or_else(|| extract_user_texts(session, 3).map(|t| truncate_str(&t, 500)));

    let tags = if session.context.tags.is_empty() {
        None
    } else {
        Some(session.context.tags.join(","))
    };

    let created_at = session.context.created_at.to_rfc3339();

    let working_directory = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str().map(String::from));

    let (files_modified, files_read, has_errors) = extract_file_metadata(session);

    UploadMetadata {
        title,
        description,
        tags,
        created_at,
        working_directory,
        files_modified,
        files_read,
        has_errors,
    }
}

/// Extract files_modified, files_read (as JSON arrays), and has_errors from a session's events.
pub fn extract_file_metadata(session: &Session) -> (Option<String>, Option<String>, bool) {
    use std::collections::BTreeSet;

    let mut modified = BTreeSet::new();
    let mut read = BTreeSet::new();
    let mut has_errors = false;

    for event in &session.events {
        match &event.event_type {
            EventType::FileEdit { path, .. }
            | EventType::FileCreate { path }
            | EventType::FileDelete { path } => {
                modified.insert(path.clone());
            }
            EventType::FileRead { path } => {
                read.insert(path.clone());
            }
            EventType::ShellCommand { exit_code, .. }
                if *exit_code != Some(0) && exit_code.is_some() =>
            {
                has_errors = true;
            }
            EventType::ToolResult { is_error: true, .. } => {
                has_errors = true;
            }
            _ => {}
        }
    }

    let read: BTreeSet<_> = read.difference(&modified).cloned().collect();

    let files_modified = if modified.is_empty() {
        None
    } else {
        let v: Vec<&String> = modified.iter().collect();
        Some(serde_json::to_string(&v).unwrap_or_default())
    };

    let files_read = if read.is_empty() {
        None
    } else {
        let v: Vec<&String> = read.iter().collect();
        Some(serde_json::to_string(&v).unwrap_or_default())
    };

    (files_modified, files_read, has_errors)
}

/// Extract the first non-empty text from a slice of content blocks.
fn extract_text_from_blocks(blocks: &[ContentBlock]) -> Option<String> {
    blocks.iter().find_map(|block| match block {
        ContentBlock::Text { text } if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => None,
    })
}

/// Extract the text from the first UserMessage event.
pub fn extract_first_user_text(session: &Session) -> Option<String> {
    session
        .events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::UserMessage))
        .find_map(|e| extract_text_from_blocks(&e.content.blocks))
}

/// Extract and join texts from the first `max` UserMessage events.
pub fn extract_user_texts(session: &Session, max: usize) -> Option<String> {
    let texts: Vec<String> = session
        .events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::UserMessage))
        .filter_map(|e| extract_text_from_blocks(&e.content.blocks))
        .take(max)
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join(" "))
    }
}

/// Extract modified and deleted file paths from a slice of events.
///
/// Returns `(modified_paths, deleted_paths)`.  Both are sorted and deduplicated.
/// If a file is deleted then re-created in the same event slice, it stays in
/// `modified` only.
pub fn extract_changed_paths(events: &[Event]) -> (Vec<String>, Vec<String>) {
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for event in events {
        match &event.event_type {
            EventType::FileEdit { path, .. } | EventType::FileCreate { path } => {
                modified.push(path.clone());
            }
            EventType::FileDelete { path } => deleted.push(path.clone()),
            _ => {}
        }
    }

    modified.sort();
    modified.dedup();
    deleted.sort();
    deleted.dedup();

    // If a file was deleted then re-created, keep it in modified
    deleted.retain(|d| !modified.contains(d));

    (modified, deleted)
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len.saturating_sub(3);
        // Don't split in the middle of a multi-byte char
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Agent, Content, Event, Session};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_session(messages: Vec<(&str, EventType)>) -> Session {
        let mut session = Session::new(
            "test".to_string(),
            Agent {
                provider: "test".to_string(),
                model: "test".to_string(),
                tool: "test".to_string(),
                tool_version: None,
            },
        );
        for (i, (text, event_type)) in messages.into_iter().enumerate() {
            session.events.push(Event {
                event_id: format!("e{i}"),
                timestamp: Utc::now(),
                event_type,
                task_id: None,
                content: Content::text(text),
                duration_ms: None,
                attributes: HashMap::new(),
            });
        }
        session
    }

    #[test]
    fn test_extract_first_user_text() {
        let session = make_session(vec![
            ("hello world", EventType::UserMessage),
            ("second message", EventType::UserMessage),
        ]);
        assert_eq!(
            extract_first_user_text(&session),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn test_extract_first_user_text_skips_agent() {
        let session = make_session(vec![
            ("agent reply", EventType::AgentMessage),
            ("user msg", EventType::UserMessage),
        ]);
        assert_eq!(
            extract_first_user_text(&session),
            Some("user msg".to_string())
        );
    }

    #[test]
    fn test_extract_first_user_text_empty() {
        let session = make_session(vec![("agent reply", EventType::AgentMessage)]);
        assert_eq!(extract_first_user_text(&session), None);
    }

    #[test]
    fn test_extract_user_texts() {
        let session = make_session(vec![
            ("first", EventType::UserMessage),
            ("reply", EventType::AgentMessage),
            ("second", EventType::UserMessage),
            ("third", EventType::UserMessage),
        ]);
        assert_eq!(
            extract_user_texts(&session, 2),
            Some("first second".to_string())
        );
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn test_extract_upload_metadata_auto_title() {
        let session = make_session(vec![
            ("Build a REST API", EventType::UserMessage),
            ("Sure, let me help", EventType::AgentMessage),
            ("Add auth too", EventType::UserMessage),
        ]);
        let meta = extract_upload_metadata(&session);
        assert_eq!(meta.title.as_deref(), Some("Build a REST API"));
        // description joins first 3 user messages
        assert_eq!(
            meta.description.as_deref(),
            Some("Build a REST API Add auth too")
        );
        assert!(meta.tags.is_none());
    }

    #[test]
    fn test_extract_upload_metadata_explicit_title() {
        let mut session = make_session(vec![("hello", EventType::UserMessage)]);
        session.context.title = Some("My Title".to_string());
        session.context.description = Some("My Desc".to_string());
        session.context.tags = vec!["rust".to_string(), "api".to_string()];

        let meta = extract_upload_metadata(&session);
        assert_eq!(meta.title.as_deref(), Some("My Title"));
        assert_eq!(meta.description.as_deref(), Some("My Desc"));
        assert_eq!(meta.tags.as_deref(), Some("rust,api"));
    }

    #[test]
    fn test_extract_changed_paths_basic() {
        let session = make_session(vec![
            (
                "edited file",
                EventType::FileEdit {
                    path: "src/main.rs".to_string(),
                    diff: None,
                },
            ),
            (
                "created file",
                EventType::FileCreate {
                    path: "src/new.rs".to_string(),
                },
            ),
            (
                "deleted file",
                EventType::FileDelete {
                    path: "src/old.rs".to_string(),
                },
            ),
            (
                "read file",
                EventType::FileRead {
                    path: "src/lib.rs".to_string(),
                },
            ),
        ]);
        let (modified, deleted) = extract_changed_paths(&session.events);
        assert_eq!(modified, vec!["src/main.rs", "src/new.rs"]);
        assert_eq!(deleted, vec!["src/old.rs"]);
    }

    #[test]
    fn test_extract_changed_paths_delete_then_recreate() {
        let session = make_session(vec![
            (
                "deleted",
                EventType::FileDelete {
                    path: "src/foo.rs".to_string(),
                },
            ),
            (
                "recreated",
                EventType::FileCreate {
                    path: "src/foo.rs".to_string(),
                },
            ),
        ]);
        let (modified, deleted) = extract_changed_paths(&session.events);
        assert_eq!(modified, vec!["src/foo.rs"]);
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_extract_changed_paths_dedup() {
        let session = make_session(vec![
            (
                "edit1",
                EventType::FileEdit {
                    path: "a.rs".to_string(),
                    diff: None,
                },
            ),
            (
                "edit2",
                EventType::FileEdit {
                    path: "a.rs".to_string(),
                    diff: None,
                },
            ),
        ]);
        let (modified, deleted) = extract_changed_paths(&session.events);
        assert_eq!(modified, vec!["a.rs"]);
        assert!(deleted.is_empty());
    }

    #[test]
    fn test_extract_upload_metadata_empty_strings() {
        let mut session = make_session(vec![("hello", EventType::UserMessage)]);
        session.context.title = Some("".to_string());
        session.context.description = Some("".to_string());

        let meta = extract_upload_metadata(&session);
        // Empty strings should trigger auto-extraction
        assert_eq!(meta.title.as_deref(), Some("hello"));
        assert_eq!(meta.description.as_deref(), Some("hello"));
    }

    #[test]
    fn test_extract_file_metadata_basic() {
        let session = make_session(vec![
            (
                "edited",
                EventType::FileEdit {
                    path: "src/main.rs".to_string(),
                    diff: None,
                },
            ),
            (
                "read",
                EventType::FileRead {
                    path: "src/lib.rs".to_string(),
                },
            ),
        ]);
        let (modified, read, has_errors) = extract_file_metadata(&session);
        assert_eq!(modified.as_deref(), Some("[\"src/main.rs\"]"));
        assert_eq!(read.as_deref(), Some("[\"src/lib.rs\"]"));
        assert!(!has_errors);
    }

    #[test]
    fn test_extract_file_metadata_read_minus_mod() {
        // If a file is both read and modified, it should only appear in modified
        let session = make_session(vec![
            (
                "read",
                EventType::FileRead {
                    path: "src/main.rs".to_string(),
                },
            ),
            (
                "edited",
                EventType::FileEdit {
                    path: "src/main.rs".to_string(),
                    diff: None,
                },
            ),
        ]);
        let (modified, read, has_errors) = extract_file_metadata(&session);
        assert_eq!(modified.as_deref(), Some("[\"src/main.rs\"]"));
        assert!(read.is_none());
        assert!(!has_errors);
    }

    #[test]
    fn test_extract_file_metadata_has_errors_cmd() {
        let session = make_session(vec![(
            "cmd",
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(1),
            },
        )]);
        let (modified, read, has_errors) = extract_file_metadata(&session);
        assert!(modified.is_none());
        assert!(read.is_none());
        assert!(has_errors);
    }

    #[test]
    fn test_extract_file_metadata_has_errors_tool() {
        let session = make_session(vec![(
            "tool err",
            EventType::ToolResult {
                name: "Bash".to_string(),
                is_error: true,
                call_id: None,
            },
        )]);
        let (_, _, has_errors) = extract_file_metadata(&session);
        assert!(has_errors);
    }

    #[test]
    fn test_extract_file_metadata_empty() {
        let session = make_session(vec![]);
        let (modified, read, has_errors) = extract_file_metadata(&session);
        assert!(modified.is_none());
        assert!(read.is_none());
        assert!(!has_errors);
    }

    #[test]
    fn test_extract_file_metadata_exit_zero() {
        let session = make_session(vec![(
            "cmd",
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(0),
            },
        )]);
        let (_, _, has_errors) = extract_file_metadata(&session);
        assert!(!has_errors);
    }
}
