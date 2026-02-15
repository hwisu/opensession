use crate::trace::{Event, EventType, Session};
use std::collections::HashSet;

fn normalize_task_id(event: &Event) -> Option<&str> {
    event
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
}

/// Returns task IDs that belong to merged/embedded Claude sub-agents.
pub fn hidden_claude_subagent_task_ids(session: &Session) -> HashSet<String> {
    if !session.agent.tool.eq_ignore_ascii_case("claude-code") {
        return HashSet::new();
    }

    let mut hidden = HashSet::new();
    for event in &session.events {
        let subagent_id = event
            .attributes
            .get("subagent_id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let is_marked_subagent = event
            .attributes
            .get("merged_subagent")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
            || subagent_id.is_some();
        if !is_marked_subagent {
            continue;
        }

        if let Some(task_id) = normalize_task_id(event) {
            hidden.insert(task_id.to_string());
        } else if let Some(task_id) = subagent_id {
            hidden.insert(task_id.to_string());
        }
    }

    hidden
}

/// Max number of concurrently active agents (main lane included).
///
/// Returns `0` for empty sessions, otherwise `>= 1`.
pub fn max_active_agents(session: &Session) -> usize {
    if session.events.is_empty() {
        return 0;
    }

    let hidden_task_ids = hidden_claude_subagent_task_ids(session);
    let mut active_task_ids: HashSet<&str> = HashSet::new();
    let mut max_subagents = 0usize;

    for event in &session.events {
        let task_id = normalize_task_id(event);
        if task_id.is_some_and(|task_id| hidden_task_ids.contains(task_id)) {
            continue;
        }

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
    use serde_json::Value;
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
    fn returns_zero_for_empty_sessions() {
        let s = session("codex", vec![]);
        assert_eq!(max_active_agents(&s), 0);
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
    fn ignores_merged_claude_subagents() {
        let mut sub = event("1", EventType::TaskStart { title: None }, Some("sub-1"));
        sub.attributes
            .insert("merged_subagent".to_string(), Value::Bool(true));
        let s = session(
            "claude-code",
            vec![
                sub,
                event("2", EventType::TaskEnd { summary: None }, Some("sub-1")),
            ],
        );
        assert_eq!(max_active_agents(&s), 1);
    }
}
