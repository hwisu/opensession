use crate::trace::{Event, EventType, Session};
use thiserror::Error;

#[derive(Debug, Error)]
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

/// Validate a complete session
pub fn validate_session(session: &Session) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    // Version check
    if !session.version.starts_with("hail-") {
        errors.push(ValidationError::InvalidVersion {
            version: session.version.clone(),
        });
    }

    // Required fields
    if session.session_id.is_empty() {
        errors.push(ValidationError::MissingField {
            field: "session_id".to_string(),
        });
    }
    if session.agent.provider.is_empty() {
        errors.push(ValidationError::MissingField {
            field: "agent.provider".to_string(),
        });
    }
    if session.agent.tool.is_empty() {
        errors.push(ValidationError::MissingField {
            field: "agent.tool".to_string(),
        });
    }

    // Events validation
    if session.events.is_empty() {
        errors.push(ValidationError::EmptySession);
    }

    let mut seen_ids = std::collections::HashSet::new();
    for (i, event) in session.events.iter().enumerate() {
        if let Err(e) = validate_event(event) {
            errors.push(ValidationError::InvalidEvent {
                index: i,
                reason: e.to_string(),
            });
        }

        // Check for duplicate event IDs
        if !seen_ids.insert(&event.event_id) {
            errors.push(ValidationError::DuplicateEventId {
                event_id: event.event_id.clone(),
            });
        }

        // Check chronological order
        if i > 0 && event.timestamp < session.events[i - 1].timestamp {
            errors.push(ValidationError::EventsOutOfOrder { index: i });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
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
        assert!(errs.iter().any(|e| matches!(e, ValidationError::EmptySession)));
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
}
