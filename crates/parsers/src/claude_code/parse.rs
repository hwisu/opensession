use super::transform::{build_cc_tool_result_content, classify_tool_use, tool_use_content};
use crate::common::{set_first, strip_system_reminders, ToolUseInfo};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{Agent, Content, Event, EventType, Session, SessionContext};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

// ── Raw JSONL deserialization types ──────────────────────────────────────────

/// Top-level entry in the Claude Code JSONL file.
/// Each line is one of these.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum RawEntry {
    #[serde(rename = "user")]
    User(RawConversationEntry),
    #[serde(rename = "assistant")]
    Assistant(RawConversationEntry),
    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {},
    #[serde(rename = "system")]
    System {},
    #[serde(rename = "progress")]
    Progress {},
    // Catch-all for unknown types we want to skip
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawConversationEntry {
    pub(crate) uuid: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    pub(crate) timestamp: String,
    pub(crate) message: RawMessage,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    agent_id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    slug: Option<String>,
    #[allow(dead_code)]
    #[serde(default, rename = "costUSD")]
    cost_usd: Option<f64>,
    #[serde(default)]
    pub(crate) usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RawUsage {
    #[serde(default)]
    pub(crate) input_tokens: u64,
    #[serde(default)]
    pub(crate) output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct RawMessage {
    pub(crate) role: String,
    pub(crate) content: RawContent,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

/// Claude Code represents user message content as either a plain string
/// or an array of content blocks.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawContent {
    Text(String),
    Blocks(Vec<RawContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum RawContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        thinking: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    // Skip unknown block types gracefully
    #[serde(other)]
    Other,
}

/// tool_result content can be a string, array of blocks, or absent
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub(crate) enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
    #[default]
    Null,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ToolResultBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

// ── Parsing logic ───────────────────────────────────────────────────────────

pub(super) fn parse_claude_code_jsonl(path: &Path) -> Result<Session> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open JSONL file: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
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
            RawEntry::FileHistorySnapshot {}
            | RawEntry::System {}
            | RawEntry::Progress {}
            | RawEntry::Unknown => continue,
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
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

    // Derive session_id from file name if not found in entries
    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
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
    if let Some(ref dir) = cwd {
        attributes.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
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
        related_session_ids: Vec::new(),
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;

    // ── Merge subagent sessions ──────────────────────────────────────────
    let session_id = session.session_id.clone();
    merge_subagent_sessions(path, &session_id, &mut session);

    session.recompute_stats();

    Ok(session)
}

fn is_subagent_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("agent-")
        || lower.starts_with("agent_")
        || lower.starts_with("subagent-")
        || lower.starts_with("subagent_")
}

fn collect_subagent_dirs(parent_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Parent default layout: `<parent-file-stem>/subagents/*.jsonl`
    dirs.push(parent_path.with_extension("").join("subagents"));

    // Fallback for legacy/alternate layouts in the same project folder.
    if let Some(parent_dir) = parent_path.parent() {
        dirs.push(parent_dir.join("subagents"));
    }

    dirs
}

fn merge_subagent_session_ids_match(
    parent_session_id: &str,
    file_name: &str,
    meta: &SubagentMeta,
) -> bool {
    if is_subagent_file_name(file_name) {
        return true;
    }

    meta.session_id
        .as_deref()
        .is_some_and(|id| id == parent_session_id)
        || meta
            .parent_session_id
            .as_deref()
            .is_some_and(|id| id == parent_session_id)
}

/// Look for likely subagent files and merge their events into the parent session.
fn merge_subagent_sessions(parent_path: &Path, parent_session_id: &str, session: &mut Session) {
    let mut subagent_files: Vec<_> = collect_subagent_dirs(parent_path)
        .into_iter()
        .filter(|dir| dir.is_dir())
        .flat_map(|dir| match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
                .collect(),
            Err(_) => Vec::new(),
        })
        .collect();

    if subagent_files.is_empty() {
        return;
    }

    subagent_files.retain(|path| {
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => return false,
        };

        if file_name.starts_with('.') {
            return false;
        }

        let meta = read_subagent_meta(path);
        if is_subagent_file_name(file_name) {
            return true;
        }

        matches!(
            meta,
            Some(meta) if merge_subagent_session_ids_match(parent_session_id, file_name, &meta)
        )
    });

    subagent_files.sort();
    if subagent_files.is_empty() {
        return;
    }

    for subagent_path in subagent_files {
        let meta = read_subagent_meta(&subagent_path).unwrap_or(SubagentMeta {
            slug: None,
            agent_id: None,
            session_id: None,
            parent_session_id: None,
        });
        let file_agent_id = subagent_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let task_id = meta
            .agent_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| file_agent_id.clone());

        // Parse the subagent JSONL (same format as parent, no recursive subagent merging)
        let sub_session = match parse_subagent_jsonl(&subagent_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse subagent {}: {}",
                    subagent_path.display(),
                    e
                );
                continue;
            }
        };

        if sub_session.events.is_empty() {
            continue;
        }
        let task_title = meta
            .slug
            .as_ref()
            .cloned()
            .unwrap_or_else(|| task_id.clone());

        let sub_model = if sub_session.agent.model != "unknown" {
            Some(sub_session.agent.model.clone())
        } else {
            None
        };

        // TaskStart event at the subagent's first event timestamp
        let start_ts = sub_session.events.first().unwrap().timestamp;
        let end_ts = sub_session.events.last().unwrap().timestamp;

        let mut start_attrs = HashMap::new();
        start_attrs.insert(
            "subagent_id".to_string(),
            serde_json::Value::String(task_id.clone()),
        );
        start_attrs.insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
        if let Some(ref model) = sub_model {
            start_attrs.insert(
                "model".to_string(),
                serde_json::Value::String(model.clone()),
            );
        }

        session.events.push(Event {
            event_id: format!("{}-start", task_id),
            timestamp: start_ts,
            event_type: EventType::TaskStart {
                title: Some(task_title),
            },
            task_id: Some(task_id.clone()),
            content: Content::text(""),
            duration_ms: None,
            attributes: start_attrs,
        });

        // Add all subagent events with task_id set
        for mut event in sub_session.events {
            event.task_id = Some(task_id.clone());
            // Prefix event_id to avoid collisions with parent
            event.event_id = format!("{}:{}", task_id, event.event_id);
            event.attributes.insert(
                "subagent_id".to_string(),
                serde_json::Value::String(task_id.clone()),
            );
            event
                .attributes
                .insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
            session.events.push(event);
        }

        // TaskEnd event
        let duration = (end_ts - start_ts).num_milliseconds().max(0) as u64;
        let mut end_attrs = HashMap::new();
        end_attrs.insert(
            "subagent_id".to_string(),
            serde_json::Value::String(task_id.clone()),
        );
        end_attrs.insert("merged_subagent".to_string(), serde_json::Value::Bool(true));
        session.events.push(Event {
            event_id: format!("{}-end", task_id),
            timestamp: end_ts,
            event_type: EventType::TaskEnd {
                summary: Some(format!(
                    "{} events, {}",
                    sub_session.stats.event_count, sub_session.agent.model
                )),
            },
            task_id: Some(task_id),
            content: Content::text(""),
            duration_ms: Some(duration),
            attributes: end_attrs,
        });
    }

    // Re-sort all events by timestamp
    session.events.sort_by_key(|e| e.timestamp);
}

/// Metadata extracted from the first line of a subagent JSONL
struct SubagentMeta {
    slug: Option<String>,
    agent_id: Option<String>,
    session_id: Option<String>,
    parent_session_id: Option<String>,
}

fn read_subagent_meta(path: &Path) -> Option<SubagentMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FirstLine {
        #[serde(default)]
        slug: Option<String>,
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default, alias = "parentUuid", alias = "parentID", alias = "parentId")]
        parent_session_id: Option<String>,
    }

    let parsed: FirstLine = serde_json::from_str(&first_line).ok()?;
    Some(SubagentMeta {
        slug: parsed.slug,
        agent_id: parsed.agent_id,
        session_id: parsed.session_id,
        parent_session_id: parsed.parent_session_id,
    })
}

/// Parse a subagent JSONL file (same format, but no recursive subagent merging)
fn parse_subagent_jsonl(path: &Path) -> Result<Session> {
    let meta = read_subagent_meta(path);
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open subagent JSONL: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut tool_use_info: HashMap<String, ToolUseInfo> = HashMap::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match entry {
            RawEntry::FileHistorySnapshot {}
            | RawEntry::System {}
            | RawEntry::Progress {}
            | RawEntry::Unknown => continue,
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_user_entry(&conv, ts, &mut events, &tool_use_info);
                }
            }
            RawEntry::Assistant(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut model_name, conv.message.model.clone());
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_assistant_entry(&conv, ts, &mut events, &mut tool_use_info);
                }
            }
        }
    }

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
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

    let context = SessionContext {
        title: None,
        description: None,
        tags: vec!["claude-code".to_string()],
        created_at,
        updated_at,
        related_session_ids: meta
            .as_ref()
            .and_then(|value| value.parent_session_id.clone())
            .filter(|value| !value.trim().is_empty())
            .into_iter()
            .collect(),
        attributes: HashMap::from([(
            "source_path".to_string(),
            serde_json::Value::String(path.to_string_lossy().to_string()),
        )]),
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
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

/// Detect Claude Code continuation/resume preamble messages
fn is_continuation_preamble(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("This session is")
        || trimmed.starts_with("Here is the conversation so far")
        || trimmed.starts_with("Here's the conversation so far")
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
                        let info = tool_use_info.get(tool_use_id).cloned().unwrap_or_else(|| {
                            ToolUseInfo {
                                name: "unknown".to_string(),
                                file_path: None,
                            }
                        });

                        let tool_name = info.name.clone();
                        let result_content = build_cc_tool_result_content(content, &info);

                        events.push(Event {
                            event_id: format!("{}-result-{}", conv.uuid, tool_use_id),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: tool_name,
                                is_error: *is_error,
                                call_id: Some(tool_use_id.clone()),
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: HashMap::new(),
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

                    events.push(Event {
                        event_id: id.clone().unwrap_or_else(|| format!("{}-tool", conv.uuid)),
                        timestamp: ts,
                        event_type,
                        task_id: None,
                        content,
                        duration_ms: None,
                        attributes: attrs.clone(),
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
            RawEntry::FileHistorySnapshot {}
            | RawEntry::System {}
            | RawEntry::Progress {}
            | RawEntry::Unknown => continue,
            RawEntry::User(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut cwd, conv.cwd.clone());
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
mod tests {
    use super::*;
    use chrono::Datelike;
    use chrono::Duration;
    use std::fs::{create_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_temp_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("opensession-claude-parser-{nanos}"));
        create_dir_all(&path).expect("create test temp root");
        path
    }

    #[test]
    fn test_parse_timestamp() {
        let ts = parse_timestamp("2026-02-06T04:46:17.839Z").unwrap();
        assert_eq!(ts.year(), 2026);
    }

    #[test]
    fn test_raw_entry_deserialization_user_string() {
        let json = r#"{"type":"user","uuid":"abc","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let entry: RawEntry = serde_json::from_str(json).unwrap();
        match entry {
            RawEntry::User(conv) => {
                assert_eq!(conv.uuid, "abc");
                match conv.message.content {
                    RawContent::Text(t) => assert_eq!(t, "hello"),
                    _ => panic!("Expected text content"),
                }
            }
            _ => panic!("Expected User entry"),
        }
    }

    #[test]
    fn test_raw_entry_deserialization_assistant() {
        let json = r#"{"type":"assistant","uuid":"def","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"assistant","model":"claude-opus-4-6","content":[{"type":"text","text":"hi"}]}}"#;
        let entry: RawEntry = serde_json::from_str(json).unwrap();
        match entry {
            RawEntry::Assistant(conv) => {
                assert_eq!(conv.message.model.as_deref(), Some("claude-opus-4-6"));
            }
            _ => panic!("Expected Assistant entry"),
        }
    }

    #[test]
    fn test_raw_entry_skip_file_history() {
        let json = r#"{"type":"file-history-snapshot","messageId":"abc","snapshot":{},"isSnapshotUpdate":false}"#;
        let entry: RawEntry = serde_json::from_str(json).unwrap();
        matches!(entry, RawEntry::FileHistorySnapshot { .. });
    }

    #[test]
    fn test_subagent_file_merge_handles_file_name_without_meta() {
        let dir = test_temp_root();
        let parent_path = dir.as_path().join("session-parent.jsonl");
        let subagent_dir = parent_path.with_extension("").join("subagents");
        create_dir_all(&subagent_dir).unwrap();

        let parent_session = "sess-parent";
        let subagent_session = "agent-abc123";

        let parent_entry = serde_json::json!({
            "type": "user",
            "uuid": "u1",
            "sessionId": parent_session,
            "timestamp": Utc::now().to_rfc3339(),
            "message": {
                "role": "user",
                "content": "parent prompt"
            }
        })
        .to_string();
        write(&parent_path, parent_entry).unwrap();

        let subagent_entry = serde_json::json!({
            "type": "assistant",
            "uuid": "a1",
            "sessionId": subagent_session,
            "timestamp": Utc::now()
                .checked_add_signed(Duration::seconds(1))
                .unwrap()
                .to_rfc3339(),
            "message": {
                "role": "assistant",
                "model": "claude-3-opus",
                "content": [{
                    "type": "text",
                    "text": "subagent reply"
                }]
            }
        })
        .to_string();
        write(
            subagent_dir.join(format!("{subagent_session}.jsonl")),
            subagent_entry,
        )
        .unwrap();

        let session = parse_claude_code_jsonl(&parent_path).unwrap();
        assert_eq!(session.events.len(), 4);
        assert!(session
            .events
            .iter()
            .any(|e| matches!(e.event_type, EventType::TaskStart { .. })));
        assert!(session.events.iter().any(|e| {
            e.attributes
                .get("merged_subagent")
                .and_then(|v| v.as_bool())
                == Some(true)
        }));
        assert!(session
            .events
            .iter()
            .any(|e| matches!(e.event_type, EventType::AgentMessage)));
        assert!(session
            .events
            .iter()
            .any(|e| matches!(e.event_type, EventType::TaskEnd { .. })));
        assert_eq!(session.stats.message_count, 2);
    }

    #[test]
    fn test_subagent_meta_reads_parent_uuid_aliases() {
        let dir = test_temp_root();
        let subagent_path = dir.as_path().join("agent-xyz.jsonl");
        let subagent_entry = serde_json::json!({
            "type": "assistant",
            "uuid": "a1",
            "sessionId": "sub-1",
            "timestamp": Utc::now().to_rfc3339(),
            "parentId": "parent-1",
            "message": {
                "role": "assistant",
                "model": "claude-3-opus",
                "content": [{
                    "type": "text",
                    "text": "sub"
                }]
            }
        })
        .to_string();
        write(&subagent_path, subagent_entry).unwrap();

        let meta = read_subagent_meta(&subagent_path).unwrap();
        assert_eq!(meta.parent_session_id.as_deref(), Some("parent-1"));
    }

    #[test]
    fn test_subagent_parse_sets_related_parent_session_id() {
        let dir = test_temp_root();
        let subagent_path = dir.as_path().join("agent-related.jsonl");
        let subagent_entry = serde_json::json!({
            "type": "assistant",
            "uuid": "a1",
            "sessionId": "sub-2",
            "timestamp": Utc::now().to_rfc3339(),
            "parentId": "parent-2",
            "message": {
                "role": "assistant",
                "model": "claude-3-opus",
                "content": [{
                    "type": "text",
                    "text": "sub"
                }]
            }
        })
        .to_string();
        write(&subagent_path, subagent_entry).unwrap();

        let parsed = parse_subagent_jsonl(&subagent_path).unwrap();
        assert_eq!(
            parsed.context.related_session_ids,
            vec!["parent-2".to_string()]
        );
    }
}
