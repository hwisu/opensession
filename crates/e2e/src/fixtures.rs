use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use opensession_api::{Agent, Content, Event, EventType, Session};

/// Create a minimal HAIL session with 3 events (UserMessage, AgentMessage, ToolCall).
pub fn minimal_session() -> Session {
    minimal_session_with_title(None)
}

/// Create a minimal HAIL session with an optional title.
pub fn minimal_session_with_title(title: Option<&str>) -> Session {
    let session_id = Uuid::new_v4().to_string();
    let mut session = Session::new(
        session_id,
        Agent {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-6".to_string(),
            tool: "claude-code".to_string(),
            tool_version: Some("1.0.0".to_string()),
        },
    );

    if let Some(t) = title {
        session.context.title = Some(t.to_string());
    }

    let now = Utc::now();
    let task_id = Uuid::new_v4().to_string();

    session.events.push(Event {
        event_id: Uuid::new_v4().to_string(),
        timestamp: now,
        event_type: EventType::UserMessage,
        task_id: Some(task_id.clone()),
        content: Content::text("Hello, write a test"),
        duration_ms: None,
        attributes: HashMap::new(),
    });

    session.events.push(Event {
        event_id: Uuid::new_v4().to_string(),
        timestamp: now + chrono::Duration::seconds(1),
        event_type: EventType::AgentMessage,
        task_id: Some(task_id.clone()),
        content: Content::text("I'll write a test for you."),
        duration_ms: None,
        attributes: HashMap::new(),
    });

    session.events.push(Event {
        event_id: Uuid::new_v4().to_string(),
        timestamp: now + chrono::Duration::seconds(2),
        event_type: EventType::ToolCall {
            name: "Write".to_string(),
        },
        task_id: Some(task_id),
        content: Content::text("Writing test file..."),
        duration_ms: Some(150),
        attributes: HashMap::new(),
    });

    session.recompute_stats();
    session
}
