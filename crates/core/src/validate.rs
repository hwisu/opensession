use crate::trace::{Event, EventType, Session};
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ValidationError {
    #[error("missing required field: {field}")]
    MissingField { field: String },
    #[error("invalid version: {version}, expected prefix 'hail-'")]
    InvalidVersion { version: String },
    #[error("empty session: no events")]
    EmptySession,
    #[error("invalid event at index {index}: {reason}")]
    InvalidEvent { index: usize, reason: String },
    #[error("events not in chronological order at index {index}")]
    EventsOutOfOrder { index: usize },
    #[error("duplicate event_id: {event_id}")]
    DuplicateEventId { event_id: String },
}

/// Validate a complete session by composing independent validators.
pub fn validate_session(session: &Session) -> Result<(), Vec<ValidationError>> {
    let validators: &[fn(&Session) -> Vec<ValidationError>] = &[
        validate_version,
        validate_required_fields,
        validate_not_empty,
        validate_events,
    ];

    let errors: Vec<ValidationError> = validators.iter().flat_map(|v| v(session)).collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_version(session: &Session) -> Vec<ValidationError> {
    if session.version.starts_with("hail-") {
        vec![]
    } else {
        vec![ValidationError::InvalidVersion {
            version: session.version.clone(),
        }]
    }
}

fn validate_required_fields(session: &Session) -> Vec<ValidationError> {
    [
        ("session_id", session.session_id.is_empty()),
        ("agent.provider", session.agent.provider.is_empty()),
        ("agent.tool", session.agent.tool.is_empty()),
    ]
    .into_iter()
    .filter(|(_, empty)| *empty)
    .map(|(field, _)| ValidationError::MissingField {
        field: field.to_string(),
    })
    .collect()
}

fn validate_not_empty(session: &Session) -> Vec<ValidationError> {
    if session.events.is_empty() {
        vec![ValidationError::EmptySession]
    } else {
        vec![]
    }
}

fn validate_events(session: &Session) -> Vec<ValidationError> {
    let individual_errors = session.events.iter().enumerate().filter_map(|(i, event)| {
        validate_event(event)
            .err()
            .map(|e| ValidationError::InvalidEvent {
                index: i,
                reason: e.to_string(),
            })
    });

    let mut seen_ids = std::collections::HashSet::new();
    let duplicate_errors = session.events.iter().filter_map(move |event| {
        if seen_ids.insert(&event.event_id) {
            None
        } else {
            Some(ValidationError::DuplicateEventId {
                event_id: event.event_id.clone(),
            })
        }
    });

    let order_errors = session
        .events
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            if pair[1].timestamp < pair[0].timestamp {
                Some(ValidationError::EventsOutOfOrder { index: i + 1 })
            } else {
                None
            }
        });

    individual_errors
        .chain(duplicate_errors)
        .chain(order_errors)
        .collect()
}

/// Validate a single event
pub fn validate_event(event: &Event) -> Result<(), ValidationError> {
    if event.event_id.is_empty() {
        return Err(ValidationError::MissingField {
            field: "event_id".to_string(),
        });
    }

    // Validate event-type-specific constraints
    match &event.event_type {
        EventType::ToolCall { name } | EventType::ToolResult { name, .. } => {
            if name.is_empty() {
                return Err(ValidationError::MissingField {
                    field: "event_type.name".to_string(),
                });
            }
        }
        EventType::FileEdit { path, .. }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path }
        | EventType::FileRead { path } => {
            if path.is_empty() {
                return Err(ValidationError::MissingField {
                    field: "event_type.path".to_string(),
                });
            }
        }
        EventType::ShellCommand { command, .. } => {
            if command.is_empty() {
                return Err(ValidationError::MissingField {
                    field: "event_type.command".to_string(),
                });
            }
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::*;
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_session_with_events(events: Vec<Event>) -> Session {
        Session {
            version: "hail-1.0.0".to_string(),
            session_id: "test-id".to_string(),
            agent: Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
            context: SessionContext::default(),
            events,
            stats: Stats::default(),
        }
    }

    #[test]
    fn test_valid_session() {
        let session = make_session_with_events(vec![Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        }]);
        assert!(validate_session(&session).is_ok());
    }

    #[test]
    fn test_empty_session() {
        let session = make_session_with_events(vec![]);
        let errs = validate_session(&session).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::EmptySession)));
    }

    #[test]
    fn test_invalid_version() {
        let mut session = make_session_with_events(vec![Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        }]);
        session.version = "bad-version".to_string();
        let errs = validate_session(&session).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidVersion { .. })));
    }

    #[test]
    fn test_duplicate_event_id() {
        let now = Utc::now();
        let session = make_session_with_events(vec![
            Event {
                event_id: "e1".to_string(),
                timestamp: now,
                event_type: EventType::UserMessage,
                task_id: None,
                content: Content::text("hello"),
                duration_ms: None,
                attributes: HashMap::new(),
            },
            Event {
                event_id: "e1".to_string(),
                timestamp: now,
                event_type: EventType::AgentMessage,
                task_id: None,
                content: Content::text("hi"),
                duration_ms: None,
                attributes: HashMap::new(),
            },
        ]);
        let errs = validate_session(&session).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateEventId { .. })));
    }

    fn make_event(id: &str, event_type: EventType) -> Event {
        Event {
            event_id: id.to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text("test"),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn test_validate_event_empty_tool_name() {
        let event = make_event(
            "e1",
            EventType::ToolCall {
                name: "".to_string(),
            },
        );
        let err = validate_event(&event).unwrap_err();
        assert!(
            matches!(err, ValidationError::MissingField { field } if field == "event_type.name")
        );
    }

    #[test]
    fn test_validate_event_empty_file_path() {
        let event = make_event(
            "e1",
            EventType::FileEdit {
                path: "".to_string(),
                diff: None,
            },
        );
        let err = validate_event(&event).unwrap_err();
        assert!(
            matches!(err, ValidationError::MissingField { field } if field == "event_type.path")
        );
    }

    #[test]
    fn test_validate_event_empty_command() {
        let event = make_event(
            "e1",
            EventType::ShellCommand {
                command: "".to_string(),
                exit_code: None,
            },
        );
        let err = validate_event(&event).unwrap_err();
        assert!(
            matches!(err, ValidationError::MissingField { field } if field == "event_type.command")
        );
    }

    #[test]
    fn test_events_out_of_order() {
        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(10);
        let session = make_session_with_events(vec![
            Event {
                event_id: "e1".to_string(),
                timestamp: now,
                event_type: EventType::UserMessage,
                task_id: None,
                content: Content::text("first"),
                duration_ms: None,
                attributes: HashMap::new(),
            },
            Event {
                event_id: "e2".to_string(),
                timestamp: earlier,
                event_type: EventType::AgentMessage,
                task_id: None,
                content: Content::text("second"),
                duration_ms: None,
                attributes: HashMap::new(),
            },
        ]);
        let errs = validate_session(&session).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::EventsOutOfOrder { index: 1 })));
    }

    #[test]
    fn test_session_id_empty() {
        let mut session = make_session_with_events(vec![make_event("e1", EventType::UserMessage)]);
        session.session_id = "".to_string();
        let errs = validate_session(&session).unwrap_err();
        assert!(errs.iter().any(
            |e| matches!(e, ValidationError::MissingField { field } if field == "session_id")
        ));
    }

    #[test]
    fn test_valid_all_event_types() {
        let now = Utc::now();
        let events: Vec<Event> = [
            EventType::UserMessage,
            EventType::AgentMessage,
            EventType::SystemMessage,
            EventType::Thinking,
            EventType::ToolCall {
                name: "Read".to_string(),
            },
            EventType::ToolResult {
                name: "Read".to_string(),
                is_error: false,
                call_id: None,
            },
            EventType::FileRead {
                path: "src/main.rs".to_string(),
            },
            EventType::CodeSearch {
                query: "fn main".to_string(),
            },
            EventType::FileSearch {
                pattern: "*.rs".to_string(),
            },
            EventType::FileEdit {
                path: "src/lib.rs".to_string(),
                diff: Some("+line".to_string()),
            },
            EventType::FileCreate {
                path: "src/new.rs".to_string(),
            },
            EventType::FileDelete {
                path: "src/old.rs".to_string(),
            },
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            EventType::ImageGenerate {
                prompt: "a cat".to_string(),
            },
            EventType::VideoGenerate {
                prompt: "a dog".to_string(),
            },
            EventType::AudioGenerate {
                prompt: "a song".to_string(),
            },
            EventType::WebSearch {
                query: "rust docs".to_string(),
            },
            EventType::WebFetch {
                url: "https://example.com".to_string(),
            },
            EventType::TaskStart {
                title: Some("task".to_string()),
            },
            EventType::TaskEnd {
                summary: Some("done".to_string()),
            },
            EventType::Custom {
                kind: "my_event".to_string(),
            },
        ]
        .into_iter()
        .enumerate()
        .map(|(i, et)| Event {
            event_id: format!("e{i}"),
            timestamp: now + chrono::Duration::milliseconds(i as i64),
            event_type: et,
            task_id: None,
            content: Content::text("test"),
            duration_ms: None,
            attributes: HashMap::new(),
        })
        .collect();

        let session = make_session_with_events(events);
        assert!(validate_session(&session).is_ok());
    }
}
