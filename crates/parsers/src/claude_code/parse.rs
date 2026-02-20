use super::transform::{build_cc_tool_result_content, classify_tool_use, tool_use_content};
use crate::common::{
    attach_semantic_attrs, attach_source_attrs, infer_tool_kind, set_first, strip_system_reminders,
    ToolUseInfo,
};
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
    System(RawSystemEntry),
    #[serde(rename = "progress")]
    Progress(RawProgressEntry),
    #[serde(rename = "queue-operation")]
    QueueOperation(RawQueueOperationEntry),
    #[serde(rename = "summary")]
    Summary(RawSummaryEntry),
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
    pub(crate) git_branch: Option<String>,
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
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSystemEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) subtype: Option<String>,
    #[serde(default)]
    pub(crate) level: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawProgressEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) data: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) tool_use_id: Option<String>,
    #[serde(default)]
    pub(crate) parent_tool_use_id: Option<String>,
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawQueueOperationEntry {
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) operation: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSummaryEntry {
    #[serde(default)]
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<String>,
    #[serde(default)]
    pub(crate) leaf_uuid: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
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
        #[serde(default)]
        tool_use_id: Option<String>,
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
    attributes.insert(
        "session_role".to_string(),
        serde_json::Value::String("primary".to_string()),
    );
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
    let mut git_branch: Option<String> = None;
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
                if let Ok(ts) = parse_timestamp(&conv.timestamp) {
                    process_user_entry(&conv, ts, &mut events, &tool_use_info);
                }
            }
            RawEntry::Assistant(conv) => {
                set_first(&mut session_id, conv.session_id.clone());
                set_first(&mut tool_version, conv.version.clone());
                set_first(&mut model_name, conv.message.model.clone());
                set_first(&mut git_branch, conv.git_branch.clone());
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

    let parent_session_id = meta
        .as_ref()
        .and_then(|value| value.parent_session_id.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut attributes = HashMap::from([(
        "source_path".to_string(),
        serde_json::Value::String(path.to_string_lossy().to_string()),
    )]);
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
    if let Some(branch) = git_branch.as_ref() {
        attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String(branch.clone()),
        );
    }

    let context = SessionContext {
        title: None,
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

fn system_entry_to_event(entry: &RawSystemEntry, events: &[Event]) -> Event {
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

fn progress_entry_to_event(entry: &RawProgressEntry, events: &[Event]) -> Event {
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

fn queue_operation_entry_to_event(entry: &RawQueueOperationEntry, events: &[Event]) -> Event {
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

fn summary_entry_to_event(entry: &RawSummaryEntry, events: &[Event]) -> Event {
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
mod tests {
    use super::*;
    use chrono::Datelike;
    use chrono::Duration;
    use std::collections::HashMap;
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
    fn test_raw_entry_deserialization_queue_operation_and_summary() {
        let queue_json = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-01-01T00:00:01Z","sessionId":"s1","content":"queued"}"#;
        let queue_entry: RawEntry = serde_json::from_str(queue_json).unwrap();
        match queue_entry {
            RawEntry::QueueOperation(entry) => {
                assert_eq!(entry.operation.as_deref(), Some("enqueue"));
                assert_eq!(entry.content.as_deref(), Some("queued"));
                assert_eq!(entry.session_id.as_deref(), Some("s1"));
            }
            _ => panic!("Expected QueueOperation entry"),
        }

        let summary_json =
            r#"{"type":"summary","summary":"Fix parser edge case","leafUuid":"leaf-1"}"#;
        let summary_entry: RawEntry = serde_json::from_str(summary_json).unwrap();
        match summary_entry {
            RawEntry::Summary(entry) => {
                assert_eq!(entry.summary.as_deref(), Some("Fix parser edge case"));
                assert_eq!(entry.leaf_uuid.as_deref(), Some("leaf-1"));
            }
            _ => panic!("Expected Summary entry"),
        }
    }

    #[test]
    fn test_parse_lines_includes_system_progress_queue_and_summary_events() {
        let lines = vec![
            serde_json::json!({
                "type": "system",
                "uuid": "sys-1",
                "sessionId": "s1",
                "timestamp": "2026-01-01T00:00:00Z",
                "gitBranch": "feature/session-branch",
                "subtype": "local_command",
                "content": "<command-name>/usage</command-name>"
            })
            .to_string(),
            serde_json::json!({
                "type": "progress",
                "uuid": "prog-1",
                "sessionId": "s1",
                "timestamp": "2026-01-01T00:00:01Z",
                "toolUseID": "tool-123",
                "data": {
                    "type": "hook_progress",
                    "hookEvent": "PreToolUse",
                    "hookName": "PreToolUse:Task"
                }
            })
            .to_string(),
            serde_json::json!({
                "type": "queue-operation",
                "sessionId": "s1",
                "timestamp": "2026-01-01T00:00:02Z",
                "operation": "enqueue",
                "content": "queued input"
            })
            .to_string(),
            serde_json::json!({
                "type": "summary",
                "sessionId": "s1",
                "leafUuid": "leaf-1",
                "summary": "Fix parser edge case"
            })
            .to_string(),
        ];

        let parsed = parse_lines_impl(&lines);
        assert_eq!(parsed.events.len(), 4);
        assert_eq!(parsed.session_id.as_deref(), Some("s1"));
        assert!(parsed
            .events
            .iter()
            .all(|event| matches!(event.event_type, EventType::SystemMessage)));

        let mut seen_raw_types = HashMap::new();
        for event in &parsed.events {
            let raw_type = event
                .attributes
                .get("source.raw_type")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            seen_raw_types.insert(raw_type, event.event_id.clone());
        }

        assert!(seen_raw_types.contains_key("system"));
        assert!(seen_raw_types.contains_key("progress"));
        assert!(seen_raw_types.contains_key("queue-operation"));
        assert!(seen_raw_types.contains_key("summary"));
        let context = parsed.context.expect("context from parsed lines");
        assert_eq!(
            context
                .attributes
                .get("git_branch")
                .and_then(|value| value.as_str()),
            Some("feature/session-branch")
        );
    }

    #[test]
    fn test_tool_result_without_tool_use_id_falls_back_to_recent_tool_use() {
        let assistant_json = r#"{
            "type":"assistant",
            "uuid":"a1",
            "sessionId":"s1",
            "timestamp":"2026-02-01T00:00:00Z",
            "message":{
                "role":"assistant",
                "model":"claude-opus-4-6",
                "content":[
                    {"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}}
                ]
            }
        }"#;
        let user_json = r#"{
            "type":"user",
            "uuid":"u1",
            "sessionId":"s1",
            "timestamp":"2026-02-01T00:00:01Z",
            "message":{
                "role":"user",
                "content":[
                    {"type":"tool_result","content":"ok","is_error":false}
                ]
            }
        }"#;

        let assistant_entry: RawEntry = serde_json::from_str(assistant_json).unwrap();
        let user_entry: RawEntry = serde_json::from_str(user_json).unwrap();
        let mut events = Vec::new();
        let mut tool_use_info = HashMap::new();

        match assistant_entry {
            RawEntry::Assistant(conv) => {
                process_assistant_entry(
                    &conv,
                    parse_timestamp(&conv.timestamp).unwrap(),
                    &mut events,
                    &mut tool_use_info,
                );
            }
            _ => panic!("expected assistant entry"),
        }
        match user_entry {
            RawEntry::User(conv) => {
                process_user_entry(
                    &conv,
                    parse_timestamp(&conv.timestamp).unwrap(),
                    &mut events,
                    &tool_use_info,
                );
            }
            _ => panic!("expected user entry"),
        }

        let result_event = events
            .iter()
            .find(|event| matches!(event.event_type, EventType::ToolResult { .. }))
            .expect("tool result exists");
        match &result_event.event_type {
            EventType::ToolResult { name, .. } => assert_eq!(name, "Read"),
            _ => unreachable!(),
        }
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
        // message_count includes user+agent messages and TaskEnd summaries.
        assert_eq!(session.stats.message_count, 3);
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
        assert_eq!(
            parsed
                .context
                .attributes
                .get("session_role")
                .and_then(|value| value.as_str()),
            Some("auxiliary")
        );
        assert_eq!(
            parsed
                .context
                .attributes
                .get("parent_session_id")
                .and_then(|value| value.as_str()),
            Some("parent-2")
        );
    }
}
