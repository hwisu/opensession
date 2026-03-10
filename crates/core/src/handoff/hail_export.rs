use std::collections::HashMap;

use crate::extract::truncate_str;
use crate::{Content, ContentBlock, Event, EventType, Session, SessionContext};

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
            attributes: HashMap::new(),
        });
    }

    summary_session.recompute_stats();
    summary_session
}
