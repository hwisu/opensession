use super::transform::{build_cc_tool_result_content, classify_tool_use, tool_use_content};
use super::{
    raw::{
        RawContent, RawContentBlock, RawConversationEntry, RawEntry, RawProgressEntry,
        RawQueueOperationEntry, RawSummaryEntry, RawSystemEntry,
    },
    subagent::{merge_subagent_sessions, read_subagent_meta},
};
use crate::common::{
    ToolUseInfo, attach_semantic_attrs, attach_source_attrs, infer_tool_kind, set_first,
    strip_system_reminders,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{Agent, Content, Event, EventType, Session, SessionContext};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

// ── Parsing logic ───────────────────────────────────────────────────────────

pub(super) fn parse_claude_code_jsonl(path: &Path) -> Result<Session> {
    let own_meta = read_subagent_meta(path);
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open JSONL file: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut first_user_text: Option<String> = None;
    let mut all_cwds: Vec<String> = Vec::new();

    // Map tool_use_id -> tool metadata (name + file_path for language detection)
    let mut tool_use_info: HashMap<String, ToolUseInfo> = HashMap::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("Failed to read JSONL line: {}", e);
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Skipping unparseable JSONL line: {}", e);
                continue;
            }
        };

        match entry {
            RawEntry::FileHistorySnapshot {} | RawEntry::Unknown => continue,
            RawEntry::System(system) => {
                set_first(&mut session_id, system.session_id.clone());
                set_first(&mut tool_version, system.version.clone());
                set_first(&mut cwd, system.cwd.clone());
                set_first(&mut git_branch, system.git_branch.clone());
                events.push(system_entry_to_event(&system, &events));
            }
            RawEntry::Progress(progress) => {
                set_first(&mut session_id, progress.session_id.clone());
                set_first(&mut tool_version, progress.version.clone());
                set_first(&mut cwd, progress.cwd.clone());
                set_first(&mut git_branch, progress.git_branch.clone());
                events.push(progress_entry_to_event(&progress, &events));
            }
            RawEntry::QueueOperation(queue_op) => {
                set_first(&mut session_id, queue_op.session_id.clone());
                events.push(queue_operation_entry_to_event(&queue_op, &events));
            }
            RawEntry::Summary(summary) => {
                set_first(&mut session_id, summary.session_id.clone());
                events.push(summary_entry_to_event(&summary, &events));
            }
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                // Collect all unique cwds for multi-repo tracking
                if let Some(ref c) = conv.cwd {
                    if !all_cwds.contains(c) {
                        all_cwds.push(c.clone());
                    }
                }
                // Capture first user message text for title
                if first_user_text.is_none() {
                    let text = match &conv.message.content {
                        RawContent::Text(t) => {
                            let cleaned = strip_system_reminders(t);
                            let trimmed = cleaned.trim();
                            if !trimmed.is_empty() && !is_continuation_preamble(trimmed) {
                                Some(trimmed.to_string())
                            } else {
                                None
                            }
                        }
                        RawContent::Blocks(blocks) => blocks.iter().find_map(|b| match b {
                            RawContentBlock::Text { text } => {
                                let cleaned = strip_system_reminders(text);
                                let trimmed = cleaned.trim();
                                if !trimmed.is_empty() && !is_continuation_preamble(trimmed) {
                                    Some(trimmed.to_string())
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }),
                    };
                    set_first(&mut first_user_text, text);
                }
                let ts = parse_timestamp(&conv.timestamp)?;
                process_user_entry(&conv, ts, &mut events, &tool_use_info);
            }
            RawEntry::Assistant(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut model_name, conv.message.model.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                if let Some(ref c) = conv.cwd {
                    if !all_cwds.contains(c) {
                        all_cwds.push(c.clone());
                    }
                }
                let ts = parse_timestamp(&conv.timestamp)?;
                process_assistant_entry(&conv, ts, &mut events, &mut tool_use_info);
            }
        }
    }

    let parent_session_id = own_meta
        .as_ref()
        .and_then(|value| value.parent_session_id.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    // Derive session_id from parsed entries, then metadata, then file name.
    let session_id = session_id.unwrap_or_else(|| {
        own_meta
            .as_ref()
            .and_then(|value| value.session_id.clone())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            })
    });

    let agent = Agent {
        provider: "anthropic".to_string(),
        model: model_name.unwrap_or_else(|| "unknown".to_string()),
        tool: "claude-code".to_string(),
        tool_version,
    };

    let (created_at, updated_at) =
        if let (Some(first), Some(last)) = (events.first(), events.last()) {
            (first.timestamp, last.timestamp)
        } else {
            let now = Utc::now();
            (now, now)
        };

    let mut attributes = HashMap::new();
    attributes.insert(
        "source_path".to_string(),
        serde_json::Value::String(path.to_string_lossy().to_string()),
    );
    attributes.insert(
        "session_role".to_string(),
        serde_json::Value::String(if parent_session_id.is_some() {
            "auxiliary".to_string()
        } else {
            "primary".to_string()
        }),
    );
    if let Some(parent_session_id) = parent_session_id.as_ref() {
        attributes.insert(
            "parent_session_id".to_string(),
            serde_json::Value::String(parent_session_id.clone()),
        );
    }
    if let Some(ref dir) = cwd {
        attributes.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
    }
    if let Some(ref branch) = git_branch {
        attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String(branch.clone()),
        );
    }
    if all_cwds.len() > 1 {
        attributes.insert(
            "all_cwds".to_string(),
            serde_json::Value::Array(
                all_cwds
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }

    let title = first_user_text.map(|t| {
        if t.chars().count() > 80 {
            let truncated: String = t.chars().take(77).collect();
            format!("{}...", truncated)
        } else {
            t
        }
    });

    let context = SessionContext {
        title,
        description: None,
        tags: vec!["claude-code".to_string()],
        created_at,
        updated_at,
        related_session_ids: parent_session_id.clone().into_iter().collect(),
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;

    // Auxiliary sessions should be hidden as child traces, not fan-in parents.
    if parent_session_id.is_none() {
        // ── Merge subagent sessions ──────────────────────────────────────
        let session_id = session.session_id.clone();
        merge_subagent_sessions(path, &session_id, &mut session);
    }

    session.recompute_stats();

    Ok(session)
}

pub(crate) fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    // Claude Code timestamps are ISO 8601, e.g. "2026-02-06T04:46:17.839Z"
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Fallback: try parsing without timezone
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
        })
        .with_context(|| format!("Failed to parse timestamp: {}", ts))
}

fn fallback_timestamp(events: &[Event]) -> DateTime<Utc> {
    events
        .last()
        .map(|event| event.timestamp)
        .unwrap_or_else(Utc::now)
}

fn parse_timestamp_with_fallback(raw: Option<&str>, fallback: DateTime<Utc>) -> DateTime<Utc> {
    raw.and_then(|ts| parse_timestamp(ts).ok())
        .unwrap_or(fallback)
}

fn event_text_or_default(raw: Option<&str>, default_text: &str) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_text.to_string())
}

fn progress_text(data: Option<&serde_json::Value>) -> String {
    let Some(data) = data else {
        return "Progress update".to_string();
    };
    let Some(obj) = data.as_object() else {
        return "Progress update".to_string();
    };
    let data_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("progress");
    if data_type == "hook_progress" {
        let hook_event = obj
            .get("hookEvent")
            .and_then(|v| v.as_str())
            .unwrap_or("hook");
        let hook_name = obj.get("hookName").and_then(|v| v.as_str());
        return match hook_name {
            Some(name) if !name.trim().is_empty() => {
                format!("Hook progress: {hook_event} ({name})")
            }
            _ => format!("Hook progress: {hook_event}"),
        };
    }
    if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
        if !message.trim().is_empty() {
            return format!("Progress: {}", message.trim());
        }
    }
    format!("Progress: {data_type}")
}

pub(super) fn system_entry_to_event(entry: &RawSystemEntry, events: &[Event]) -> Event {
    let fallback = fallback_timestamp(events);
    let timestamp = parse_timestamp_with_fallback(entry.timestamp.as_deref(), fallback);
    let subtype = entry.subtype.as_deref().unwrap_or("unknown");
    let default_text = format!("System event: {subtype}");
    let text = event_text_or_default(entry.content.as_deref(), &default_text);

    let mut attrs = HashMap::new();
    attach_source_attrs(&mut attrs, Some("claude-code-jsonl-v1"), Some("system"));
    attach_semantic_attrs(&mut attrs, entry.uuid.as_deref(), None, None);
    if let Some(subtype) = entry.subtype.as_deref().filter(|v| !v.trim().is_empty()) {
        attrs.insert(
            "system.subtype".to_string(),
            serde_json::Value::String(subtype.to_string()),
        );
    }
    if let Some(level) = entry.level.as_deref().filter(|v| !v.trim().is_empty()) {
        attrs.insert(
            "system.level".to_string(),
            serde_json::Value::String(level.to_string()),
        );
    }

    let event_id = entry
        .uuid
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("system-{}", timestamp.timestamp_millis()));

    Event {
        event_id,
        timestamp,
        event_type: EventType::SystemMessage,
        task_id: None,
        content: Content::text(text),
        duration_ms: None,
        attributes: attrs,
    }
}

pub(super) fn progress_entry_to_event(entry: &RawProgressEntry, events: &[Event]) -> Event {
    let fallback = fallback_timestamp(events);
    let timestamp = parse_timestamp_with_fallback(entry.timestamp.as_deref(), fallback);
    let mut attrs = HashMap::new();
    attach_source_attrs(&mut attrs, Some("claude-code-jsonl-v1"), Some("progress"));
    let call_id = entry
        .tool_use_id
        .as_deref()
        .or(entry.parent_tool_use_id.as_deref());
    attach_semantic_attrs(&mut attrs, entry.uuid.as_deref(), call_id, None);

    if let Some(data_type) = entry
        .data
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
    {
        attrs.insert(
            "progress.type".to_string(),
            serde_json::Value::String(data_type.to_string()),
        );
    }

    let event_id = entry
        .uuid
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("progress-{}", timestamp.timestamp_millis()));

    Event {
        event_id,
        timestamp,
        event_type: EventType::SystemMessage,
        task_id: None,
        content: Content::text(progress_text(entry.data.as_ref())),
        duration_ms: None,
        attributes: attrs,
    }
}

pub(super) fn queue_operation_entry_to_event(
    entry: &RawQueueOperationEntry,
    events: &[Event],
) -> Event {
    let fallback = fallback_timestamp(events);
    let timestamp = parse_timestamp_with_fallback(entry.timestamp.as_deref(), fallback);
    let operation = entry
        .operation
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let text = match (operation, entry.content.as_deref()) {
        ("enqueue", Some(content)) if !content.trim().is_empty() => {
            format!("Queued input: {}", content.trim())
        }
        _ => format!("Queue operation: {operation}"),
    };

    let mut attrs = HashMap::new();
    attach_source_attrs(
        &mut attrs,
        Some("claude-code-jsonl-v1"),
        Some("queue-operation"),
    );
    attach_semantic_attrs(&mut attrs, None, None, None);
    attrs.insert(
        "queue.operation".to_string(),
        serde_json::Value::String(operation.to_string()),
    );

    let event_id = format!("queue-{}-{operation}", timestamp.timestamp_millis());
    Event {
        event_id,
        timestamp,
        event_type: EventType::SystemMessage,
        task_id: None,
        content: Content::text(text),
        duration_ms: None,
        attributes: attrs,
    }
}

pub(super) fn summary_entry_to_event(entry: &RawSummaryEntry, events: &[Event]) -> Event {
    let fallback = fallback_timestamp(events);
    let timestamp = parse_timestamp_with_fallback(entry.timestamp.as_deref(), fallback);
    let summary_text = event_text_or_default(entry.summary.as_deref(), "Summary");
    let text = if entry
        .summary
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        summary_text
    } else {
        format!("Summary: {summary_text}")
    };

    let mut attrs = HashMap::new();
    attach_source_attrs(&mut attrs, Some("claude-code-jsonl-v1"), Some("summary"));
    let group_id = entry.uuid.as_deref().or(entry.leaf_uuid.as_deref());
    attach_semantic_attrs(&mut attrs, group_id, None, None);
    if let Some(leaf_uuid) = entry
        .leaf_uuid
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        attrs.insert(
            "summary.leaf_uuid".to_string(),
            serde_json::Value::String(leaf_uuid.to_string()),
        );
    }

    let event_id = entry
        .uuid
        .clone()
        .or_else(|| entry.leaf_uuid.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("summary-{}", timestamp.timestamp_millis()));

    Event {
        event_id,
        timestamp,
        event_type: EventType::SystemMessage,
        task_id: None,
        content: Content::text(text),
        duration_ms: None,
        attributes: attrs,
    }
}

/// Detect Claude Code continuation/resume preamble messages
fn is_continuation_preamble(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("This session is")
        || trimmed.starts_with("Here is the conversation so far")
        || trimmed.starts_with("Here's the conversation so far")
}

fn fallback_tool_info_from_recent_events(
    events: &[Event],
) -> Option<(ToolUseInfo, Option<String>)> {
    for event in events.iter().rev() {
        let Some(tool_name) = event
            .attributes
            .get("tool_use_name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let file_path = match &event.event_type {
            EventType::FileRead { path }
            | EventType::FileCreate { path }
            | EventType::FileDelete { path } => Some(path.clone()),
            EventType::FileEdit { path, .. } => Some(path.clone()),
            _ => None,
        };
        let call_id = event
            .attributes
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        return Some((
            ToolUseInfo {
                name: tool_name,
                file_path,
            },
            call_id,
        ));
    }
    None
}

pub(crate) fn process_user_entry(
    conv: &RawConversationEntry,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    tool_use_info: &HashMap<String, ToolUseInfo>,
) {
    match &conv.message.content {
        RawContent::Text(text) => {
            let cleaned = strip_system_reminders(text);
            if !cleaned.trim().is_empty() {
                let event_type = if is_continuation_preamble(&cleaned) {
                    EventType::SystemMessage
                } else {
                    EventType::UserMessage
                };
                events.push(Event {
                    event_id: conv.uuid.clone(),
                    timestamp: ts,
                    event_type,
                    task_id: None,
                    content: Content::text(cleaned),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
        }
        RawContent::Blocks(blocks) => {
            for block in blocks {
                match block {
                    RawContentBlock::Text { text } => {
                        let cleaned = strip_system_reminders(text);
                        if !cleaned.trim().is_empty() {
                            let event_type = if is_continuation_preamble(&cleaned) {
                                EventType::SystemMessage
                            } else {
                                EventType::UserMessage
                            };
                            events.push(Event {
                                event_id: format!("{}-text", conv.uuid),
                                timestamp: ts,
                                event_type,
                                task_id: None,
                                content: Content::text(cleaned),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    RawContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let fallback = fallback_tool_info_from_recent_events(events);
                        let info = tool_use_id
                            .as_deref()
                            .and_then(|id| tool_use_info.get(id).cloned())
                            .or_else(|| fallback.as_ref().map(|(info, _)| info.clone()))
                            .unwrap_or_else(|| ToolUseInfo {
                                name: "unknown".to_string(),
                                file_path: None,
                            });
                        let resolved_call_id = tool_use_id
                            .clone()
                            .or_else(|| fallback.as_ref().and_then(|(_, call_id)| call_id.clone()));
                        let tool_name = info.name.clone();
                        let result_content = build_cc_tool_result_content(content, &info);
                        let mut attrs = HashMap::new();
                        attach_source_attrs(
                            &mut attrs,
                            Some("claude-code-jsonl-v1"),
                            Some("tool_result"),
                        );
                        attach_semantic_attrs(
                            &mut attrs,
                            Some(&conv.uuid),
                            resolved_call_id.as_deref(),
                            Some(infer_tool_kind(&tool_name)),
                        );

                        events.push(Event {
                            event_id: format!(
                                "{}-result-{}",
                                conv.uuid,
                                resolved_call_id.as_deref().unwrap_or("fallback")
                            ),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: tool_name,
                                is_error: *is_error,
                                call_id: resolved_call_id,
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: attrs,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

pub(crate) fn process_assistant_entry(
    conv: &RawConversationEntry,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    tool_use_info: &mut HashMap<String, ToolUseInfo>,
) {
    // Build per-event attributes with model info
    let mut attrs = HashMap::new();
    if let Some(ref model) = conv.message.model {
        attrs.insert(
            "model".to_string(),
            serde_json::Value::String(model.clone()),
        );
    }

    // Add token usage data (attached to first AgentMessage for this turn)
    let mut token_attrs = HashMap::new();
    if let Some(ref usage) = conv.usage {
        if usage.input_tokens > 0 {
            token_attrs.insert(
                "input_tokens".to_string(),
                serde_json::Value::Number(usage.input_tokens.into()),
            );
        }
        if usage.output_tokens > 0 {
            token_attrs.insert(
                "output_tokens".to_string(),
                serde_json::Value::Number(usage.output_tokens.into()),
            );
        }
    }
    let mut tokens_emitted = false;

    if let RawContent::Blocks(blocks) = &conv.message.content {
        for block in blocks {
            match block {
                RawContentBlock::Text { text } => {
                    let cleaned = strip_system_reminders(text);
                    if cleaned.is_empty() {
                        continue;
                    }
                    let mut event_attrs = attrs.clone();
                    if !tokens_emitted && !token_attrs.is_empty() {
                        event_attrs.extend(token_attrs.clone());
                        tokens_emitted = true;
                    }
                    events.push(Event {
                        event_id: format!("{}-text", conv.uuid),
                        timestamp: ts,
                        event_type: EventType::AgentMessage,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: event_attrs,
                    });
                }
                RawContentBlock::Thinking { thinking } => {
                    let text = thinking.as_deref().unwrap_or("");
                    let cleaned = strip_system_reminders(text);
                    if cleaned.is_empty() {
                        continue;
                    }
                    events.push(Event {
                        event_id: format!("{}-thinking", conv.uuid),
                        timestamp: ts,
                        event_type: EventType::Thinking,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: attrs.clone(),
                    });
                }
                RawContentBlock::ToolUse { id, name, input } => {
                    // Extract file_path from tool input for language detection in ToolResult
                    let file_path = match name.as_str() {
                        "Read" | "Write" | "Edit" | "NotebookEdit" => input
                            .get("file_path")
                            .or_else(|| input.get("notebook_path"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        "Grep" => input
                            .get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    };

                    // Track tool_use_id -> info for matching ToolResults
                    if let Some(tool_id) = id {
                        tool_use_info.insert(
                            tool_id.clone(),
                            ToolUseInfo {
                                name: name.clone(),
                                file_path,
                            },
                        );
                    }

                    let event_type = classify_tool_use(name, input);
                    let content = tool_use_content(name, input);
                    let mut event_attrs = attrs.clone();
                    attach_source_attrs(
                        &mut event_attrs,
                        Some("claude-code-jsonl-v1"),
                        Some("tool_use"),
                    );
                    attach_semantic_attrs(
                        &mut event_attrs,
                        Some(&conv.uuid),
                        id.as_deref(),
                        Some(infer_tool_kind(name)),
                    );
                    event_attrs.insert(
                        "tool_use_name".to_string(),
                        serde_json::Value::String(name.clone()),
                    );
                    if let Some(tool_id) = id.as_deref() {
                        event_attrs.insert(
                            "tool_use_id".to_string(),
                            serde_json::Value::String(tool_id.to_string()),
                        );
                    }

                    events.push(Event {
                        event_id: id.clone().unwrap_or_else(|| format!("{}-tool", conv.uuid)),
                        timestamp: ts,
                        event_type,
                        task_id: None,
                        content,
                        duration_ms: None,
                        attributes: event_attrs,
                    });
                }
                _ => {}
            }
        }
    }
}

// ── Incremental line parsing (for stream-push) ──────────────────────────────

/// Result of parsing a batch of Claude Code JSONL lines.
pub(super) struct ParsedLines {
    pub agent: Option<Agent>,
    pub context: Option<SessionContext>,
    pub events: Vec<Event>,
    pub session_id: Option<String>,
}

/// Parse raw Claude Code JSONL lines into structured HAIL components.
///
/// This is the incremental counterpart of `parse_claude_code_jsonl()`:
/// it processes pre-read lines without opening a file and is lenient
/// with malformed lines (skips them instead of returning errors).
pub(super) fn parse_lines_impl(lines: &[String]) -> ParsedLines {
    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut first_user_text: Option<String> = None;
    let mut all_cwds: Vec<String> = Vec::new();
    let mut tool_use_info: HashMap<String, ToolUseInfo> = HashMap::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match entry {
            RawEntry::FileHistorySnapshot {} | RawEntry::Unknown => continue,
            RawEntry::System(system) => {
                set_first(&mut session_id, system.session_id.clone());
                set_first(&mut tool_version, system.version.clone());
                set_first(&mut cwd, system.cwd.clone());
                set_first(&mut git_branch, system.git_branch.clone());
                events.push(system_entry_to_event(&system, &events));
            }
            RawEntry::Progress(progress) => {
                set_first(&mut session_id, progress.session_id.clone());
                set_first(&mut tool_version, progress.version.clone());
                set_first(&mut cwd, progress.cwd.clone());
                set_first(&mut git_branch, progress.git_branch.clone());
                events.push(progress_entry_to_event(&progress, &events));
            }
            RawEntry::QueueOperation(queue_op) => {
                set_first(&mut session_id, queue_op.session_id.clone());
                events.push(queue_operation_entry_to_event(&queue_op, &events));
            }
            RawEntry::Summary(summary) => {
                set_first(&mut session_id, summary.session_id.clone());
                events.push(summary_entry_to_event(&summary, &events));
            }
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                if let Some(ref c) = conv.cwd {
                    if !all_cwds.contains(c) {
                        all_cwds.push(c.clone());
                    }
                }
                if first_user_text.is_none() {
                    let text = match &conv.message.content {
                        RawContent::Text(t) => {
                            let cleaned = strip_system_reminders(t);
                            let trimmed = cleaned.trim();
                            if !trimmed.is_empty() && !is_continuation_preamble(trimmed) {
                                Some(trimmed.to_string())
                            } else {
                                None
                            }
                        }
                        RawContent::Blocks(blocks) => blocks.iter().find_map(|b| match b {
                            RawContentBlock::Text { text } => {
                                let cleaned = strip_system_reminders(text);
                                let trimmed = cleaned.trim();
                                if !trimmed.is_empty() && !is_continuation_preamble(trimmed) {
                                    Some(trimmed.to_string())
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        }),
                    };
                    set_first(&mut first_user_text, text);
                }
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_user_entry(&conv, ts, &mut events, &tool_use_info);
                }
            }
            RawEntry::Assistant(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut model_name, conv.message.model.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
                if let Some(ref c) = conv.cwd {
                    if !all_cwds.contains(c) {
                        all_cwds.push(c.clone());
                    }
                }
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_assistant_entry(&conv, ts, &mut events, &mut tool_use_info);
                }
            }
        }
    }

    let agent = if model_name.is_some() || tool_version.is_some() {
        Some(Agent {
            provider: "anthropic".to_string(),
            model: model_name.unwrap_or_else(|| "unknown".to_string()),
            tool: "claude-code".to_string(),
            tool_version,
        })
    } else {
        None
    };

    let context = if !events.is_empty() {
        let created_at = events.first().unwrap().timestamp;
        let updated_at = events.last().unwrap().timestamp;
        let mut attributes = HashMap::new();
        if let Some(ref dir) = cwd {
            attributes.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
        }
        if let Some(ref branch) = git_branch {
            attributes.insert(
                "git_branch".to_string(),
                serde_json::Value::String(branch.clone()),
            );
        }
        if all_cwds.len() > 1 {
            attributes.insert(
                "all_cwds".to_string(),
                serde_json::Value::Array(
                    all_cwds
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        let title = first_user_text.map(|t| {
            if t.chars().count() > 80 {
                let truncated: String = t.chars().take(77).collect();
                format!("{}...", truncated)
            } else {
                t
            }
        });
        Some(SessionContext {
            title,
            description: None,
            tags: vec!["claude-code".to_string()],
            created_at,
            updated_at,
            related_session_ids: Vec::new(),
            attributes,
        })
    } else {
        None
    };

    ParsedLines {
        agent,
        context,
        events,
        session_id,
    }
}

#[cfg(test)]
mod tests;
