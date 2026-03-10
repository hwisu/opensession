use super::*;

pub(super) fn remove_duplicate_response_fallback(
    events: &mut Vec<Event>,
    ts: DateTime<Utc>,
    text: &str,
) {
    let normalized = normalize_user_text_for_dedupe(text);
    events.retain(|event| {
        if !matches!(event.event_type, EventType::UserMessage) {
            return true;
        }
        if event
            .attributes
            .get("source")
            .and_then(|value| value.as_str())
            != Some("response_fallback")
        {
            return true;
        }
        if (event.timestamp - ts).num_seconds().abs() > 12 {
            return true;
        }
        event_user_text(event)
            .map(|existing| !user_texts_equivalent(&existing, &normalized))
            .unwrap_or(true)
    });
}

pub(super) fn remove_duplicate_agent_response_fallback(
    events: &mut Vec<Event>,
    ts: DateTime<Utc>,
    text: &str,
) {
    let normalized = normalize_user_text_for_dedupe(text);
    events.retain(|event| {
        if !matches!(event.event_type, EventType::AgentMessage) {
            return true;
        }
        if event
            .attributes
            .get("source")
            .and_then(|value| value.as_str())
            != Some("response_fallback")
        {
            return true;
        }
        if (event.timestamp - ts).num_seconds().abs() > 12 {
            return true;
        }
        event_agent_text(event)
            .map(|existing| !user_texts_equivalent(&existing, &normalized))
            .unwrap_or(true)
    });
}

pub(super) fn should_skip_duplicate_user_event(
    events: &[Event],
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) -> bool {
    let source = match source {
        Some(source) => source,
        None => return false,
    };
    let opposite = match opposite_dedupe_source(source) {
        Some(opposite) => opposite,
        None => return false,
    };
    let normalized = normalize_user_text_for_dedupe(text);
    events.iter().any(|event| {
        if !matches!(event.event_type, EventType::UserMessage) {
            return false;
        }
        let event_source = event
            .attributes
            .get("source")
            .and_then(|value| value.as_str());
        if event_source != Some(opposite) && event_source != Some(source) {
            return false;
        }
        let duplicate_window_secs = if event_source == Some(source) { 2 } else { 12 };
        if (event.timestamp - ts).num_seconds().abs() > duplicate_window_secs {
            return false;
        }
        event_user_text(event)
            .map(|existing| user_texts_equivalent(&existing, &normalized))
            .unwrap_or(false)
    })
}

pub(super) fn should_skip_duplicate_agent_event(
    events: &[Event],
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) -> bool {
    let source = match source {
        Some(source) => source,
        None => return false,
    };
    let opposite = match opposite_dedupe_source(source) {
        Some(opposite) => opposite,
        None => return false,
    };
    let normalized = normalize_user_text_for_dedupe(text);
    events.iter().any(|event| {
        if !matches!(event.event_type, EventType::AgentMessage) {
            return false;
        }
        let event_source = event
            .attributes
            .get("source")
            .and_then(|value| value.as_str());
        if event_source != Some(opposite) && event_source != Some(source) {
            return false;
        }
        let duplicate_window_secs = if event_source == Some(source) { 2 } else { 12 };
        if (event.timestamp - ts).num_seconds().abs() > duplicate_window_secs {
            return false;
        }
        event_agent_text(event)
            .map(|existing| user_texts_equivalent(&existing, &normalized))
            .unwrap_or(false)
    })
}

pub(super) fn opposite_dedupe_source(source: &str) -> Option<&'static str> {
    match source {
        "event_msg" => Some("response_fallback"),
        "response_fallback" => Some("event_msg"),
        _ => None,
    }
}

pub(super) fn event_user_text(event: &Event) -> Option<String> {
    if !matches!(event.event_type, EventType::UserMessage) {
        return None;
    }
    let mut out = Vec::new();
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

pub(super) fn event_agent_text(event: &Event) -> Option<String> {
    if !matches!(event.event_type, EventType::AgentMessage) {
        return None;
    }
    let mut out = Vec::new();
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

pub(super) fn normalize_user_text_for_dedupe(text: &str) -> String {
    let normalized = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !matches!(
                lower.as_str(),
                "<image>" | "<file>" | "<audio>" | "<video>" | "[image]" | "[file]"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

pub(super) fn user_texts_equivalent(lhs: &str, rhs: &str) -> bool {
    let left = normalize_user_text_for_dedupe(lhs);
    let right = normalize_user_text_for_dedupe(rhs);
    if left == right {
        return true;
    }

    let min_len = left.chars().count().min(right.chars().count());
    min_len >= 16 && (left.contains(&right) || right.contains(&left))
}

#[allow(dead_code)]
pub(super) fn normalize_user_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}
