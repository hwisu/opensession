use crate::trace::{Event, EventType, Session};
use std::collections::HashSet;

fn normalize_task_id(event: &Event) -> Option<&str> {
    event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
}

/// Max number of concurrently active agents (main lane included).
///
/// Always returns `>= 1` (main lane baseline).
pub fn max_active_agents(session: &Session) -> usize {
    if session.events.is_empty() {
        return 1;
    }

    let mut active_task_ids: HashSet<&str> = HashSet::new();
    let mut max_subagents = 0usize;

    for event in &session.events {
        let task_id = normalize_task_id(event);

        if matches!(event.event_type, EventType::TaskStart { .. }) {
            if let Some(task_id) = task_id {
                active_task_ids.insert(task_id);
            }
        }

        max_subagents = max_subagents.max(active_task_ids.len());

        if matches!(event.event_type, EventType::TaskEnd { .. }) {
            if let Some(task_id) = task_id {
                active_task_ids.remove(task_id);
            }
        }
    }

    max_subagents + 1
}

#[cfg(test)]
mod tests {
    use super::max_active_agents;
    use crate::trace::{Agent, Content, Event, EventType, Session};
    use chrono::Utc;
    use std::collections::HashMap;

    fn event(id: &str, event_type: EventType, task_id: Option<&str>) -> Event {
        Event {
            event_id: id.to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: task_id.map(ToString::to_string),
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    fn session(tool: &str, events: Vec<Event>) -> Session {
        Session {
            version: "hail-1.0.0".to_string(),
            session_id: "s1".to_string(),
            agent: Agent {
                provider: "p".to_string(),
                model: "m".to_string(),
                tool: tool.to_string(),
                tool_version: None,
            },
            context: crate::trace::SessionContext {
                title: None,
                description: None,
                tags: vec![],
                created_at: Utc::now(),
                updated_at: Utc::now(),
                related_session_ids: vec![],
                attributes: HashMap::new(),
            },
            events,
            stats: Default::default(),
        }
    }

    #[test]
    fn returns_one_for_empty_sessions() {
        let s = session("codex", vec![]);
        assert_eq!(max_active_agents(&s), 1);
    }

    #[test]
    fn counts_concurrent_tasks() {
        let s = session(
            "codex",
            vec![
                event("1", EventType::TaskStart { title: None }, Some("t1")),
                event("2", EventType::TaskStart { title: None }, Some("t2")),
                event("3", EventType::TaskEnd { summary: None }, Some("t2")),
                event("4", EventType::TaskEnd { summary: None }, Some("t1")),
            ],
        );
        assert_eq!(max_active_agents(&s), 3);
    }

    #[test]
    fn counts_merged_claude_subagents_for_agent_concurrency() {
        let sub = event("1", EventType::TaskStart { title: None }, Some("sub-1"));
        let s = session(
            "claude-code",
            vec![
                sub,
                event("2", EventType::TaskEnd { summary: None }, Some("sub-1")),
            ],
        );
        assert_eq!(max_active_agents(&s), 2);
    }
}
