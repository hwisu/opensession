//! Handoff context generation — extract structured summaries from sessions.
//!
//! This module provides programmatic extraction of session summaries for handoff
//! between agent sessions. It supports both single-session and multi-session merge.

use std::collections::{HashMap, HashSet};

use crate::extract::truncate_str;
use crate::{Content, ContentBlock, Event, EventType, Session, SessionContext, Stats};

// ─── Types ───────────────────────────────────────────────────────────────────

/// A file change observed during a session.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    /// "created" | "edited" | "deleted"
    pub action: &'static str,
}

/// A shell command executed during a session.
#[derive(Debug, Clone)]
pub struct ShellCmd {
    pub command: String,
    pub exit_code: Option<i32>,
}

/// A user→agent conversation pair.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub user: String,
    pub agent: String,
}

/// Summary extracted from a single session.
#[derive(Debug, Clone)]
pub struct HandoffSummary {
    pub source_session_id: String,
    pub objective: String,
    pub tool: String,
    pub model: String,
    pub duration_seconds: u64,
    pub stats: Stats,
    pub files_modified: Vec<FileChange>,
    pub files_read: Vec<String>,
    pub shell_commands: Vec<ShellCmd>,
    pub errors: Vec<String>,
    pub task_summaries: Vec<String>,
    pub key_conversations: Vec<Conversation>,
    pub user_messages: Vec<String>,
}

/// Merged handoff from multiple sessions.
#[derive(Debug, Clone)]
pub struct MergedHandoff {
    pub source_session_ids: Vec<String>,
    pub summaries: Vec<HandoffSummary>,
    /// Deduplicated union of all modified files
    pub all_files_modified: Vec<FileChange>,
    /// Deduplicated union of all files read (minus modified)
    pub all_files_read: Vec<String>,
    pub total_duration_seconds: u64,
    pub total_errors: Vec<String>,
}

// ─── Extraction ──────────────────────────────────────────────────────────────

impl HandoffSummary {
    /// Extract a structured summary from a parsed session.
    pub fn from_session(session: &Session) -> Self {
        let objective = extract_objective(session);

        let files_modified = collect_file_changes(&session.events);
        let modified_paths: HashSet<&str> =
            files_modified.iter().map(|f| f.path.as_str()).collect();
        let files_read = collect_files_read(&session.events, &modified_paths);
        let shell_commands = collect_shell_commands(&session.events);
        let errors = collect_errors(&session.events);
        let task_summaries = collect_task_summaries(&session.events);
        let user_messages = collect_user_messages(&session.events);
        let key_conversations = collect_conversation_pairs(&session.events);

        HandoffSummary {
            source_session_id: session.session_id.clone(),
            objective,
            tool: session.agent.tool.clone(),
            model: session.agent.model.clone(),
            duration_seconds: session.stats.duration_seconds,
            stats: session.stats.clone(),
            files_modified,
            files_read,
            shell_commands,
            errors,
            task_summaries,
            key_conversations,
            user_messages,
        }
    }
}

// ─── Functional extractors ──────────────────────────────────────────────────

/// Collect file changes, preserving create/delete precedence over edits.
fn collect_file_changes(events: &[Event]) -> Vec<FileChange> {
    let map = events.iter().fold(HashMap::new(), |mut map, event| {
        match &event.event_type {
            EventType::FileCreate { path } => {
                map.insert(path.clone(), "created");
            }
            EventType::FileEdit { path, .. } => {
                map.entry(path.clone()).or_insert("edited");
            }
            EventType::FileDelete { path } => {
                map.insert(path.clone(), "deleted");
            }
            _ => {}
        }
        map
    });
    let mut result: Vec<FileChange> = map
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// Collect read-only file paths (excluding those that were also modified).
fn collect_files_read(events: &[Event], modified_paths: &HashSet<&str>) -> Vec<String> {
    let mut read: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.event_type {
            EventType::FileRead { path } if !modified_paths.contains(path.as_str()) => {
                Some(path.clone())
            }
            _ => None,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    read.sort();
    read
}

fn collect_shell_commands(events: &[Event]) -> Vec<ShellCmd> {
    events
        .iter()
        .filter_map(|event| match &event.event_type {
            EventType::ShellCommand { command, exit_code } => Some(ShellCmd {
                command: command.clone(),
                exit_code: *exit_code,
            }),
            _ => None,
        })
        .collect()
}

/// Collect errors from failed shell commands and tool results.
fn collect_errors(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.event_type {
            EventType::ShellCommand { command, exit_code }
                if *exit_code != Some(0) && exit_code.is_some() =>
            {
                Some(format!(
                    "Shell: `{}` → exit {}",
                    truncate_str(command, 80),
                    exit_code.unwrap()
                ))
            }
            EventType::ToolResult {
                is_error: true,
                name,
                ..
            } => {
                let detail = extract_text_from_event(event);
                Some(match detail {
                    Some(d) => format!("Tool error: {} — {}", name, truncate_str(&d, 80)),
                    None => format!("Tool error: {name}"),
                })
            }
            _ => None,
        })
        .collect()
}

fn collect_task_summaries(events: &[Event]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut summaries = Vec::new();

    for event in events {
        let EventType::TaskEnd {
            summary: Some(summary),
        } = &event.event_type
        else {
            continue;
        };

        let summary = summary.trim();
        if summary.is_empty() {
            continue;
        }

        let normalized = collapse_whitespace(summary);
        if normalized.eq_ignore_ascii_case("synthetic end (missing task_complete)") {
            continue;
        }
        if seen.insert(normalized.clone()) {
            summaries.push(truncate_str(&normalized, 180));
        }
    }

    summaries
}

fn collect_user_messages(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter(|e| matches!(&e.event_type, EventType::UserMessage))
        .filter_map(extract_text_from_event)
        .collect()
}

/// Pair adjacent User→Agent messages into conversations.
///
/// Filters to message events only, then uses `windows(2)` to find
/// UserMessage→AgentMessage pairs — no mutable tracking state needed.
fn collect_conversation_pairs(events: &[Event]) -> Vec<Conversation> {
    let messages: Vec<&Event> = events
        .iter()
        .filter(|e| {
            matches!(
                &e.event_type,
                EventType::UserMessage | EventType::AgentMessage
            )
        })
        .collect();

    messages
        .windows(2)
        .filter_map(|pair| match (&pair[0].event_type, &pair[1].event_type) {
            (EventType::UserMessage, EventType::AgentMessage) => {
                let user_text = extract_text_from_event(pair[0])?;
                let agent_text = extract_text_from_event(pair[1])?;
                Some(Conversation {
                    user: truncate_str(&user_text, 300),
                    agent: truncate_str(&agent_text, 300),
                })
            }
            _ => None,
        })
        .collect()
}

// ─── Merge ───────────────────────────────────────────────────────────────────

/// Merge multiple session summaries into a single handoff context.
pub fn merge_summaries(summaries: &[HandoffSummary]) -> MergedHandoff {
    let session_ids: Vec<String> = summaries
        .iter()
        .map(|s| s.source_session_id.clone())
        .collect();
    let total_duration: u64 = summaries.iter().map(|s| s.duration_seconds).sum();
    let total_errors: Vec<String> = summaries
        .iter()
        .flat_map(|s| {
            s.errors
                .iter()
                .map(move |err| format!("[{}] {}", s.source_session_id, err))
        })
        .collect();

    let all_modified: HashMap<String, &str> = summaries
        .iter()
        .flat_map(|s| &s.files_modified)
        .fold(HashMap::new(), |mut map, fc| {
            map.entry(fc.path.clone()).or_insert(fc.action);
            map
        });

    // Compute sorted_read before consuming all_modified
    let mut sorted_read: Vec<String> = summaries
        .iter()
        .flat_map(|s| &s.files_read)
        .filter(|p| !all_modified.contains_key(p.as_str()))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sorted_read.sort();

    let mut sorted_modified: Vec<FileChange> = all_modified
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    sorted_modified.sort_by(|a, b| a.path.cmp(&b.path));

    MergedHandoff {
        source_session_ids: session_ids,
        summaries: summaries.to_vec(),
        all_files_modified: sorted_modified,
        all_files_read: sorted_read,
        total_duration_seconds: total_duration,
        total_errors,
    }
}

// ─── Markdown generation ─────────────────────────────────────────────────────

/// Generate a Markdown handoff document from a single session summary.
pub fn generate_handoff_markdown(summary: &HandoffSummary) -> String {
    const MAX_TASK_SUMMARIES_DISPLAY: usize = 5;
    let mut md = String::new();

    md.push_str("# Session Handoff\n\n");

    // Objective
    md.push_str("## Objective\n");
    md.push_str(&summary.objective);
    md.push_str("\n\n");

    // Summary
    md.push_str("## Summary\n");
    md.push_str(&format!(
        "- **Tool:** {} ({})\n",
        summary.tool, summary.model
    ));
    md.push_str(&format!(
        "- **Duration:** {}\n",
        format_duration(summary.duration_seconds)
    ));
    md.push_str(&format!(
        "- **Messages:** {} | Tool calls: {} | Events: {}\n",
        summary.stats.message_count, summary.stats.tool_call_count, summary.stats.event_count
    ));
    md.push('\n');

    if !summary.task_summaries.is_empty() {
        md.push_str("## Task Summaries\n");
        for (idx, task_summary) in summary
            .task_summaries
            .iter()
            .take(MAX_TASK_SUMMARIES_DISPLAY)
            .enumerate()
        {
            md.push_str(&format!("{}. {}\n", idx + 1, task_summary));
        }
        if summary.task_summaries.len() > MAX_TASK_SUMMARIES_DISPLAY {
            md.push_str(&format!(
                "- ... and {} more\n",
                summary.task_summaries.len() - MAX_TASK_SUMMARIES_DISPLAY
            ));
        }
        md.push('\n');
    }

    // Files Modified
    if !summary.files_modified.is_empty() {
        md.push_str("## Files Modified\n");
        for fc in &summary.files_modified {
            md.push_str(&format!("- `{}` ({})\n", fc.path, fc.action));
        }
        md.push('\n');
    }

    // Files Read
    if !summary.files_read.is_empty() {
        md.push_str("## Files Read\n");
        for path in &summary.files_read {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    // Shell Commands
    if !summary.shell_commands.is_empty() {
        md.push_str("## Shell Commands\n");
        for cmd in &summary.shell_commands {
            let code_str = match cmd.exit_code {
                Some(c) => c.to_string(),
                None => "?".to_string(),
            };
            md.push_str(&format!(
                "- `{}` → {}\n",
                truncate_str(&cmd.command, 80),
                code_str
            ));
        }
        md.push('\n');
    }

    // Errors
    if !summary.errors.is_empty() {
        md.push_str("## Errors\n");
        for err in &summary.errors {
            md.push_str(&format!("- {err}\n"));
        }
        md.push('\n');
    }

    // Key Conversations (user + agent pairs)
    if !summary.key_conversations.is_empty() {
        md.push_str("## Key Conversations\n");
        for (i, conv) in summary.key_conversations.iter().enumerate() {
            md.push_str(&format!(
                "### {}. User\n{}\n\n### {}. Agent\n{}\n\n",
                i + 1,
                truncate_str(&conv.user, 300),
                i + 1,
                truncate_str(&conv.agent, 300),
            ));
        }
    }

    // User Messages (fallback list)
    if summary.key_conversations.is_empty() && !summary.user_messages.is_empty() {
        md.push_str("## User Messages\n");
        for (i, msg) in summary.user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", i + 1, truncate_str(msg, 150)));
        }
        md.push('\n');
    }

    md
}

/// Generate a Markdown handoff document from a merged multi-session handoff.
pub fn generate_merged_handoff_markdown(merged: &MergedHandoff) -> String {
    const MAX_TASK_SUMMARIES_DISPLAY: usize = 3;
    let mut md = String::new();

    md.push_str("# Merged Session Handoff\n\n");
    md.push_str(&format!(
        "**Sessions:** {} | **Total Duration:** {}\n\n",
        merged.source_session_ids.len(),
        format_duration(merged.total_duration_seconds)
    ));

    // Per-session summaries
    for (i, s) in merged.summaries.iter().enumerate() {
        md.push_str(&format!(
            "---\n\n## Session {} — {}\n\n",
            i + 1,
            s.source_session_id
        ));
        md.push_str(&format!("**Objective:** {}\n\n", s.objective));
        md.push_str(&format!(
            "- **Tool:** {} ({}) | **Duration:** {}\n",
            s.tool,
            s.model,
            format_duration(s.duration_seconds)
        ));
        md.push_str(&format!(
            "- **Messages:** {} | Tool calls: {} | Events: {}\n\n",
            s.stats.message_count, s.stats.tool_call_count, s.stats.event_count
        ));

        if !s.task_summaries.is_empty() {
            md.push_str("### Task Summaries\n");
            for (j, task_summary) in s
                .task_summaries
                .iter()
                .take(MAX_TASK_SUMMARIES_DISPLAY)
                .enumerate()
            {
                md.push_str(&format!("{}. {}\n", j + 1, task_summary));
            }
            if s.task_summaries.len() > MAX_TASK_SUMMARIES_DISPLAY {
                md.push_str(&format!(
                    "- ... and {} more\n",
                    s.task_summaries.len() - MAX_TASK_SUMMARIES_DISPLAY
                ));
            }
            md.push('\n');
        }

        // Key Conversations for this session
        if !s.key_conversations.is_empty() {
            md.push_str("### Conversations\n");
            for (j, conv) in s.key_conversations.iter().enumerate() {
                md.push_str(&format!(
                    "**{}. User:** {}\n\n**{}. Agent:** {}\n\n",
                    j + 1,
                    truncate_str(&conv.user, 200),
                    j + 1,
                    truncate_str(&conv.agent, 200),
                ));
            }
        }
    }

    // Combined files
    md.push_str("---\n\n## All Files Modified\n");
    if merged.all_files_modified.is_empty() {
        md.push_str("_(none)_\n");
    } else {
        for fc in &merged.all_files_modified {
            md.push_str(&format!("- `{}` ({})\n", fc.path, fc.action));
        }
    }
    md.push('\n');

    if !merged.all_files_read.is_empty() {
        md.push_str("## All Files Read\n");
        for path in &merged.all_files_read {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    // Errors
    if !merged.total_errors.is_empty() {
        md.push_str("## All Errors\n");
        for err in &merged.total_errors {
            md.push_str(&format!("- {err}\n"));
        }
        md.push('\n');
    }

    md
}

// ─── Summary HAIL generation ─────────────────────────────────────────────────

/// Generate a summary HAIL session from an original session.
///
/// Filters events to only include important ones and truncates content.
pub fn generate_handoff_hail(session: &Session) -> Session {
    let mut summary_session = Session {
        version: session.version.clone(),
        session_id: format!("handoff-{}", session.session_id),
        agent: session.agent.clone(),
        context: SessionContext {
            title: Some(format!(
                "Handoff: {}",
                session.context.title.as_deref().unwrap_or("(untitled)")
            )),
            description: session.context.description.clone(),
            tags: {
                let mut tags = session.context.tags.clone();
                if !tags.contains(&"handoff".to_string()) {
                    tags.push("handoff".to_string());
                }
                tags
            },
            created_at: session.context.created_at,
            updated_at: chrono::Utc::now(),
            related_session_ids: vec![session.session_id.clone()],
            attributes: HashMap::new(),
        },
        events: Vec::new(),
        stats: session.stats.clone(),
    };

    for event in &session.events {
        let keep = matches!(
            &event.event_type,
            EventType::UserMessage
                | EventType::AgentMessage
                | EventType::FileEdit { .. }
                | EventType::FileCreate { .. }
                | EventType::FileDelete { .. }
                | EventType::TaskStart { .. }
                | EventType::TaskEnd { .. }
        ) || matches!(&event.event_type, EventType::ShellCommand { exit_code, .. } if *exit_code != Some(0));

        if !keep {
            continue;
        }

        // Truncate content blocks
        let truncated_blocks: Vec<ContentBlock> = event
            .content
            .blocks
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => ContentBlock::Text {
                    text: truncate_str(text, 300),
                },
                ContentBlock::Code {
                    code,
                    language,
                    start_line,
                } => ContentBlock::Code {
                    code: truncate_str(code, 300),
                    language: language.clone(),
                    start_line: *start_line,
                },
                other => other.clone(),
            })
            .collect();

        summary_session.events.push(Event {
            event_id: event.event_id.clone(),
            timestamp: event.timestamp,
            event_type: event.event_type.clone(),
            task_id: event.task_id.clone(),
            content: Content {
                blocks: truncated_blocks,
            },
            duration_ms: event.duration_ms,
            attributes: HashMap::new(), // strip detailed attributes
        });
    }

    // Recompute stats for the filtered events
    summary_session.recompute_stats();

    summary_session
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn extract_first_user_text(session: &Session) -> Option<String> {
    crate::extract::extract_first_user_text(session)
}

fn extract_objective(session: &Session) -> String {
    if let Some(user_text) = extract_first_user_text(session).filter(|t| !t.trim().is_empty()) {
        return truncate_str(&collapse_whitespace(&user_text), 200);
    }

    if let Some(task_title) = session
        .events
        .iter()
        .find_map(|event| match &event.event_type {
            EventType::TaskStart { title: Some(title) } => {
                let title = title.trim();
                if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                }
            }
            _ => None,
        })
    {
        return truncate_str(&collapse_whitespace(&task_title), 200);
    }

    if let Some(task_summary) = session
        .events
        .iter()
        .find_map(|event| match &event.event_type {
            EventType::TaskEnd {
                summary: Some(summary),
            } => {
                let summary = summary.trim();
                if summary.is_empty() {
                    None
                } else {
                    Some(summary.to_string())
                }
            }
            _ => None,
        })
    {
        return truncate_str(&collapse_whitespace(&task_summary), 200);
    }

    if let Some(title) = session.context.title.as_deref().map(str::trim) {
        if !title.is_empty() {
            return truncate_str(&collapse_whitespace(title), 200);
        }
    }

    "(objective unavailable)".to_string()
}

fn extract_text_from_event(event: &Event) -> Option<String> {
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Format seconds into a human-readable duration string.
pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m}m {s}s")
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        let s = seconds % 60;
        format!("{h}h {m}m {s}s")
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing, Agent};

    fn make_agent() -> Agent {
        testing::agent()
    }

    fn make_event(event_type: EventType, text: &str) -> Event {
        testing::event(event_type, text)
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(750), "12m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_handoff_summary_from_session() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            duration_seconds: 750,
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "I'll fix it now"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "Cargo.toml".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Build now passes in local env".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);

        assert_eq!(summary.source_session_id, "test-id");
        assert_eq!(summary.objective, "Fix the build error");
        assert_eq!(summary.files_modified.len(), 1);
        assert_eq!(summary.files_modified[0].path, "src/main.rs");
        assert_eq!(summary.files_modified[0].action, "edited");
        assert_eq!(summary.files_read, vec!["Cargo.toml"]);
        assert_eq!(summary.shell_commands.len(), 1);
        assert_eq!(
            summary.task_summaries,
            vec!["Build now passes in local env".to_string()]
        );
        assert_eq!(summary.key_conversations.len(), 1);
        assert_eq!(summary.key_conversations[0].user, "Fix the build error");
        assert_eq!(summary.key_conversations[0].agent, "I'll fix it now");
    }

    #[test]
    fn test_handoff_objective_falls_back_to_task_title() {
        let mut session = Session::new("task-title-fallback".to_string(), make_agent());
        session.context.title = Some("session-019c-example.jsonl".to_string());
        session.events.push(make_event(
            EventType::TaskStart {
                title: Some("Refactor auth middleware for oauth callback".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(
            summary.objective,
            "Refactor auth middleware for oauth callback"
        );
    }

    #[test]
    fn test_handoff_task_summaries_are_deduplicated() {
        let mut session = Session::new("task-summary-dedupe".to_string(), make_agent());
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Add worker profile guard".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some(" ".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Add worker profile guard".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Hide teams nav for worker profile".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(
            summary.task_summaries,
            vec![
                "Add worker profile guard".to_string(),
                "Hide teams nav for worker profile".to_string()
            ]
        );
    }

    #[test]
    fn test_files_read_excludes_modified() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "src/main.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "README.md".to_string(),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.files_read, vec!["README.md"]);
        assert_eq!(summary.files_modified.len(), 1);
    }

    #[test]
    fn test_file_create_not_overwritten_by_edit() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileCreate {
                path: "new_file.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "new_file.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.files_modified[0].action, "created");
    }

    #[test]
    fn test_shell_error_captured() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        assert_eq!(summary.errors.len(), 1);
        assert!(summary.errors[0].contains("cargo test"));
    }

    #[test]
    fn test_generate_handoff_markdown() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            duration_seconds: 750,
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "I'll fix it now"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::TaskEnd {
                summary: Some("Compile error fixed by updating trait bounds".to_string()),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

        assert!(md.contains("# Session Handoff"));
        assert!(md.contains("Fix the build error"));
        assert!(md.contains("claude-code (claude-opus-4-6)"));
        assert!(md.contains("12m 30s"));
        assert!(md.contains("## Task Summaries"));
        assert!(md.contains("Compile error fixed by updating trait bounds"));
        assert!(md.contains("`src/main.rs` (edited)"));
        assert!(md.contains("`cargo build` → 0"));
        assert!(md.contains("## Key Conversations"));
    }

    #[test]
    fn test_merge_summaries() {
        let mut s1 = Session::new("session-a".to_string(), make_agent());
        s1.stats.duration_seconds = 100;
        s1.events.push(make_event(EventType::UserMessage, "task A"));
        s1.events.push(make_event(
            EventType::FileEdit {
                path: "a.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let mut s2 = Session::new("session-b".to_string(), make_agent());
        s2.stats.duration_seconds = 200;
        s2.events.push(make_event(EventType::UserMessage, "task B"));
        s2.events.push(make_event(
            EventType::FileEdit {
                path: "b.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let sum1 = HandoffSummary::from_session(&s1);
        let sum2 = HandoffSummary::from_session(&s2);
        let merged = merge_summaries(&[sum1, sum2]);

        assert_eq!(merged.source_session_ids.len(), 2);
        assert_eq!(merged.total_duration_seconds, 300);
        assert_eq!(merged.all_files_modified.len(), 2);
    }

    #[test]
    fn test_generate_handoff_hail() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "Hello"));
        session
            .events
            .push(make_event(EventType::AgentMessage, "Hi there"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "foo.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "foo.rs".to_string(),
                diff: Some("+added line".to_string()),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));

        let hail = generate_handoff_hail(&session);

        assert!(hail.session_id.starts_with("handoff-"));
        assert_eq!(hail.context.related_session_ids, vec!["test-id"]);
        assert!(hail.context.tags.contains(&"handoff".to_string()));
        // FileRead and successful ShellCommand should be filtered out
        assert_eq!(hail.events.len(), 3); // UserMessage, AgentMessage, FileEdit
                                          // Verify HAIL roundtrip
        let jsonl = hail.to_jsonl().unwrap();
        let parsed = Session::from_jsonl(&jsonl).unwrap();
        assert_eq!(parsed.session_id, hail.session_id);
    }
}
