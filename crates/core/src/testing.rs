use crate::{Agent, Content, Event, EventType};
use std::collections::HashMap;

/// Default agent for tests (anthropic / claude-opus-4-6 / claude-code).
pub fn agent() -> Agent {
    Agent {
        provider: "anthropic".to_string(),
        model: "claude-opus-4-6".to_string(),
        tool: "claude-code".to_string(),
        tool_version: None,
    }
}

/// Agent with custom tool and model (provider = "test").
pub fn agent_with(tool: &str, model: &str) -> Agent {
    Agent {
        provider: "test".to_string(),
        model: model.to_string(),
        tool: tool.to_string(),
        tool_version: None,
    }
}

/// Simple text event.
pub fn event(event_type: EventType, text: &str) -> Event {
    Event {
        event_id: format!("test-{}", next_id()),
        timestamp: chrono::Utc::now(),
        event_type,
        task_id: None,
        content: Content::text(text),
        duration_ms: None,
        attributes: HashMap::new(),
    }
}

/// Event with arbitrary [`Content`].
pub fn event_with_content(event_type: EventType, content: Content) -> Event {
    Event {
        event_id: format!("test-{}", next_id()),
        timestamp: chrono::Utc::now(),
        event_type,
        task_id: None,
        content,
        duration_ms: None,
        attributes: HashMap::new(),
    }
}

fn next_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
