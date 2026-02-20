use crate::common::{attach_semantic_attrs, attach_source_attrs, infer_tool_kind, set_first};
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use opensession_core::trace::{Agent, Content, Event, EventType, Session, SessionContext};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub struct OpenCodeParser;

impl SessionParser for OpenCodeParser {
    fn name(&self) -> &str {
        "opencode"
    }

    fn can_parse(&self, path: &Path) -> bool {
        // Actual layout: ~/.local/share/opencode/storage/session/<project_hash>/<session_id>.json
        path.extension().is_some_and(|ext| ext == "json")
            && path
                .to_str()
                .is_some_and(|s| s.contains("opencode") && s.contains("/storage/session/"))
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_opencode_session(path)
    }
}

// ── Deserialization types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(
        rename = "parentID",
        alias = "parentId",
        alias = "parent_id",
        alias = "parentUUID",
        alias = "parent_uuid",
        alias = "parentSessionID",
        alias = "parentSessionId",
        alias = "parentSessionUuid",
        alias = "parent_session_uuid",
        default
    )]
    parent_id: Option<String>,
    #[serde(default)]
    time: Option<TimeRange>,
    #[serde(default)]
    directory: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TimeRange {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    updated: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct MessageInfo {
    id: String,
    role: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, rename = "providerID", alias = "providerId")]
    provider_id: Option<String>,
    #[serde(default, rename = "modelID", alias = "modelId")]
    model_id: Option<String>,
    #[serde(default)]
    model: Option<ModelRef>,
    #[serde(default)]
    time: Option<MessageTime>,
    #[serde(default)]
    cost: Option<f64>,
    #[serde(default)]
    tokens: Option<serde_json::Value>,
}

/// Nested model reference: { "providerID": "openai", "modelID": "gpt-5.2-codex" }
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ModelRef {
    #[serde(default, rename = "providerID")]
    provider_id: Option<String>,
    #[serde(default, rename = "modelID")]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MessageTime {
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct PartInfo {
    id: String,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default, rename = "callID")]
    call_id: Option<String>,
    #[serde(rename = "type")]
    part_type: String,
    // text parts
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    files: Option<Vec<String>>,
    #[serde(default)]
    hash: Option<String>,
    // tool parts
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    state: Option<ToolState>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    // time
    #[serde(default)]
    time: Option<PartTime>,
}

#[derive(Debug, Deserialize)]
struct PartTime {
    #[serde(default)]
    start: Option<u64>,
    #[serde(default)]
    end: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ToolState {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    output: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    time: Option<PartTime>,
}

// ── Parsing logic ───────────────────────────────────────────────────────────

fn parse_opencode_session(info_path: &Path) -> Result<Session> {
    // Read session info
    let info_text = std::fs::read_to_string(info_path)
        .with_context(|| format!("Failed to read session info: {}", info_path.display()))?;
    let info: SessionInfo = serde_json::from_str(&info_text)
        .with_context(|| format!("Failed to parse session info: {}", info_path.display()))?;

    // Actual layout:
    //   info_path:  .../storage/session/<project_hash>/<session_id>.json
    //   messages:   .../storage/message/<session_id>/<msg_id>.json
    //   parts:      .../storage/part/<msg_id>/<part_id>.json
    let storage_dir = info_path
        .parent() // session/<project_hash>/
        .and_then(|p| p.parent()) // session/
        .and_then(|p| p.parent()) // storage/
        .ok_or_else(|| anyhow::anyhow!("Invalid info path structure"))?;

    let message_dir = storage_dir.join("message").join(&info.id);
    let part_base_dir = storage_dir.join("part");

    // Read all messages
    let mut messages: Vec<MessageInfo> = Vec::new();
    if message_dir.exists() {
        for entry in std::fs::read_dir(&message_dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "json") {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    if let Ok(msg) = serde_json::from_str::<MessageInfo>(&text) {
                        messages.push(msg);
                    }
                }
            }
        }
    }

    // Sort messages by creation time
    messages.sort_by_key(|m| m.time.as_ref().and_then(|t| t.created).unwrap_or(0));

    // Read parts for each message (parts are stored at storage/part/<msg_id>/)
    let mut parts_by_message: HashMap<String, Vec<PartInfo>> = HashMap::new();
    for msg in &messages {
        let msg_part_dir = resolve_part_dir_for_message(&part_base_dir, &msg.id);
        let Some(msg_part_dir) = msg_part_dir else {
            continue;
        };
        let mut parts: Vec<PartInfo> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&msg_part_dir) {
            for part_entry in entries {
                let part_entry = match part_entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if part_entry.path().extension().is_some_and(|e| e == "json") {
                    if let Ok(text) = std::fs::read_to_string(part_entry.path()) {
                        if let Ok(part) = serde_json::from_str::<PartInfo>(&text) {
                            parts.push(part);
                        }
                    }
                }
            }
        }
        // Sort parts by start time
        parts.sort_by_key(|p| {
            p.time
                .as_ref()
                .and_then(|t| t.start)
                .or_else(|| {
                    p.state
                        .as_ref()
                        .and_then(|s| s.time.as_ref().and_then(|t| t.start))
                })
                .unwrap_or(0)
        });
        parts_by_message.insert(msg.id.clone(), parts);
    }

    // Convert to HAIL events
    let mut events: Vec<Event> = Vec::new();
    let mut model_id: Option<String> = None;
    let mut provider_id: Option<String> = None;
    let mut open_tasks: HashMap<String, (DateTime<Utc>, String)> = HashMap::new();
    let schema_version = info.version.as_deref().unwrap_or("opencode-unknown");

    for msg in &messages {
        // Extract model/provider from either top-level fields or nested model object
        set_first(&mut model_id, msg.model_id.clone());
        set_first(&mut provider_id, msg.provider_id.clone());
        if let Some(ref model_ref) = msg.model {
            set_first(&mut model_id, model_ref.model_id.clone());
            set_first(&mut provider_id, model_ref.provider_id.clone());
        }

        let msg_ts = msg
            .time
            .as_ref()
            .and_then(|t| t.created)
            .map(millis_to_datetime)
            .unwrap_or_else(Utc::now);
        let mut message_attrs = HashMap::new();
        attach_source_attrs(&mut message_attrs, Some(schema_version), Some("message"));
        attach_semantic_attrs(&mut message_attrs, Some(&msg.id), None, None);

        // Process parts for this message
        if let Some(parts) = parts_by_message.get(&msg.id) {
            for part in parts {
                let part_ts = part
                    .time
                    .as_ref()
                    .and_then(|t| t.start)
                    .or_else(|| {
                        part.state
                            .as_ref()
                            .and_then(|s| s.time.as_ref().and_then(|t| t.start))
                    })
                    .map(millis_to_datetime)
                    .unwrap_or(msg_ts);

                let duration_ms = part
                    .time
                    .as_ref()
                    .and_then(|t| {
                        let start = t.start?;
                        let end = t.end?;
                        Some(end.saturating_sub(start))
                    })
                    .or_else(|| {
                        part.state.as_ref().and_then(|s| {
                            s.time.as_ref().and_then(|t| {
                                let start = t.start?;
                                let end = t.end?;
                                Some(end.saturating_sub(start))
                            })
                        })
                    });

                match part.part_type.as_str() {
                    "text" => {
                        let text = part.text.as_deref().unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let event_type = match msg.role.as_str() {
                            "user" => EventType::UserMessage,
                            "assistant" => EventType::AgentMessage,
                            _ => continue,
                        };
                        let mut attrs = message_attrs.clone();
                        attach_source_attrs(&mut attrs, Some(schema_version), Some("part:text"));
                        events.push(Event {
                            event_id: part.id.clone(),
                            timestamp: part_ts,
                            event_type,
                            task_id: None,
                            content: Content::text(text),
                            duration_ms,
                            attributes: attrs,
                        });
                    }
                    "reasoning" => {
                        let raw_reasoning = part.text.as_deref().unwrap_or("").trim();
                        let reasoning_text = if !raw_reasoning.is_empty() {
                            Some(raw_reasoning.to_string())
                        } else if reasoning_has_encrypted_payload(part.metadata.as_ref()) {
                            Some("Encrypted reasoning".to_string())
                        } else {
                            None
                        };
                        let Some(reasoning_text) = reasoning_text else {
                            continue;
                        };
                        let mut attrs = message_attrs.clone();
                        attach_source_attrs(
                            &mut attrs,
                            Some(schema_version),
                            Some("part:reasoning"),
                        );
                        events.push(Event {
                            event_id: part.id.clone(),
                            timestamp: part_ts,
                            event_type: EventType::Thinking,
                            task_id: None,
                            content: Content::text(reasoning_text),
                            duration_ms,
                            attributes: attrs,
                        });
                    }
                    "file" => {
                        let descriptor = part
                            .filename
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .or_else(|| {
                                part.url
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                            })
                            .or_else(|| {
                                part.text
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                            })
                            .unwrap_or("attached file");
                        let event_type = match msg.role.as_str() {
                            "user" => EventType::UserMessage,
                            "assistant" => EventType::AgentMessage,
                            _ => continue,
                        };
                        let mut attrs = message_attrs.clone();
                        attach_source_attrs(&mut attrs, Some(schema_version), Some("part:file"));
                        events.push(Event {
                            event_id: part.id.clone(),
                            timestamp: part_ts,
                            event_type,
                            task_id: None,
                            content: Content::text(format!("Attached file: {descriptor}")),
                            duration_ms,
                            attributes: attrs,
                        });
                    }
                    "tool" => {
                        let tool_name = part.tool.as_deref().unwrap_or("unknown").to_string();
                        let tool_kind = infer_tool_kind(&tool_name);
                        let state = part.state.as_ref();
                        let status = state
                            .and_then(|s| s.status.as_deref())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(|v| v.to_ascii_lowercase())
                            .unwrap_or_else(|| "unknown".to_string());
                        let semantic_call_id = normalized_call_id(part.call_id.as_deref());
                        let task_id = semantic_call_id
                            .clone()
                            .unwrap_or_else(|| format!("opencode-task-{}", part.id));
                        let title = state
                            .and_then(|s| s.title.as_deref())
                            .filter(|v| !v.trim().is_empty())
                            .map(str::to_string)
                            .or_else(|| Some(tool_name.clone()));

                        let mut start_attrs = message_attrs.clone();
                        attach_source_attrs(
                            &mut start_attrs,
                            Some(schema_version),
                            Some("part:tool-task-start"),
                        );
                        attach_semantic_attrs(
                            &mut start_attrs,
                            Some(&msg.id),
                            semantic_call_id.as_deref(),
                            Some(tool_kind),
                        );
                        events.push(Event {
                            event_id: format!("{}-task-start", part.id),
                            timestamp: part_ts,
                            event_type: EventType::TaskStart { title },
                            task_id: Some(task_id.clone()),
                            content: Content::empty(),
                            duration_ms: None,
                            attributes: start_attrs,
                        });
                        open_tasks.insert(task_id.clone(), (part_ts, part.id.clone()));

                        // Emit ToolCall
                        let input = state.and_then(|s| s.input.clone());
                        let event_type = classify_opencode_tool(&tool_name, &input);
                        let content = opencode_tool_content(&tool_name, &input);
                        let call_event_id = format!("{}-call", part.id);
                        let correlated_call_id = semantic_call_id
                            .clone()
                            .unwrap_or_else(|| call_event_id.clone());
                        let mut call_attrs = message_attrs.clone();
                        attach_source_attrs(
                            &mut call_attrs,
                            Some(schema_version),
                            Some("part:tool-call"),
                        );
                        attach_semantic_attrs(
                            &mut call_attrs,
                            Some(&msg.id),
                            Some(&correlated_call_id),
                            Some(tool_kind),
                        );

                        events.push(Event {
                            event_id: call_event_id.clone(),
                            timestamp: part_ts,
                            event_type,
                            task_id: Some(task_id.clone()),
                            content,
                            duration_ms,
                            attributes: call_attrs,
                        });

                        // Emit ToolResult for terminal tool outputs.
                        if is_result_tool_status(&status) {
                            let output_text = extract_tool_output_text(state).unwrap_or_default();
                            let mut result_attrs = message_attrs.clone();
                            attach_source_attrs(
                                &mut result_attrs,
                                Some(schema_version),
                                Some("part:tool-result"),
                            );
                            attach_semantic_attrs(
                                &mut result_attrs,
                                Some(&msg.id),
                                Some(&correlated_call_id),
                                Some(tool_kind),
                            );

                            events.push(Event {
                                event_id: format!("{}-result", part.id),
                                timestamp: part_ts,
                                event_type: EventType::ToolResult {
                                    name: tool_name.clone(),
                                    is_error: status == "error" || status == "failed",
                                    call_id: Some(correlated_call_id),
                                },
                                task_id: Some(task_id.clone()),
                                content: Content::text(&output_text),
                                duration_ms: None,
                                attributes: result_attrs,
                            });
                        }

                        if is_terminal_tool_status(&status) {
                            let summary = state
                                .and_then(|s| s.title.as_deref())
                                .filter(|v| !v.trim().is_empty())
                                .map(str::to_string)
                                .or_else(|| {
                                    if status == "error" || status == "failed" {
                                        Some(format!("{tool_name} failed"))
                                    } else {
                                        Some(format!("{tool_name} {status}"))
                                    }
                                });
                            let mut end_attrs = message_attrs.clone();
                            attach_source_attrs(
                                &mut end_attrs,
                                Some(schema_version),
                                Some("part:tool-task-end"),
                            );
                            attach_semantic_attrs(
                                &mut end_attrs,
                                Some(&msg.id),
                                semantic_call_id.as_deref(),
                                Some(tool_kind),
                            );
                            events.push(Event {
                                event_id: format!("{}-task-end", part.id),
                                timestamp: part_ts,
                                event_type: EventType::TaskEnd { summary },
                                task_id: Some(task_id.clone()),
                                content: Content::empty(),
                                duration_ms: None,
                                attributes: end_attrs,
                            });
                            open_tasks.remove(&task_id);
                        }
                    }
                    "patch" => {
                        const MAX_PATCH_FILE_EVENTS: usize = 8;
                        let files: Vec<String> = part
                            .files
                            .as_deref()
                            .unwrap_or(&[])
                            .iter()
                            .map(|path| path.trim())
                            .filter(|path| !path.is_empty())
                            .map(str::to_string)
                            .collect();
                        let patch_hash = part
                            .hash
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty());

                        if !files.is_empty() && files.len() <= MAX_PATCH_FILE_EVENTS {
                            for (idx, path) in files.iter().enumerate() {
                                let mut attrs = message_attrs.clone();
                                attach_source_attrs(
                                    &mut attrs,
                                    Some(schema_version),
                                    Some("part:patch:file"),
                                );
                                attach_semantic_attrs(
                                    &mut attrs,
                                    Some(&msg.id),
                                    None,
                                    Some("file_write"),
                                );
                                if let Some(hash) = patch_hash {
                                    attrs.insert(
                                        "patch.hash".to_string(),
                                        serde_json::Value::String(hash.to_string()),
                                    );
                                }
                                events.push(Event {
                                    event_id: format!("{}-patch-{}", part.id, idx + 1),
                                    timestamp: part_ts,
                                    event_type: EventType::FileEdit {
                                        path: path.to_string(),
                                        diff: None,
                                    },
                                    task_id: None,
                                    content: Content::empty(),
                                    duration_ms,
                                    attributes: attrs,
                                });
                            }
                        } else {
                            let mut attrs = message_attrs.clone();
                            attach_source_attrs(
                                &mut attrs,
                                Some(schema_version),
                                Some("part:patch:summary"),
                            );
                            attach_semantic_attrs(
                                &mut attrs,
                                Some(&msg.id),
                                None,
                                Some("file_write"),
                            );
                            if let Some(hash) = patch_hash {
                                attrs.insert(
                                    "patch.hash".to_string(),
                                    serde_json::Value::String(hash.to_string()),
                                );
                            }
                            let mut summary = serde_json::Map::new();
                            summary.insert(
                                "file_count".to_string(),
                                serde_json::Value::from(files.len() as u64),
                            );
                            if let Some(hash) = patch_hash {
                                summary.insert(
                                    "hash".to_string(),
                                    serde_json::Value::String(hash.to_string()),
                                );
                            }
                            if !files.is_empty() {
                                summary.insert(
                                    "files_preview".to_string(),
                                    serde_json::Value::Array(
                                        files
                                            .iter()
                                            .take(MAX_PATCH_FILE_EVENTS)
                                            .cloned()
                                            .map(serde_json::Value::String)
                                            .collect(),
                                    ),
                                );
                                summary.insert(
                                    "truncated".to_string(),
                                    serde_json::Value::Bool(files.len() > MAX_PATCH_FILE_EVENTS),
                                );
                            }
                            events.push(Event {
                                event_id: format!("{}-patch", part.id),
                                timestamp: part_ts,
                                event_type: EventType::Custom {
                                    kind: "patch".to_string(),
                                },
                                task_id: None,
                                content: Content {
                                    blocks: vec![opensession_core::trace::ContentBlock::Json {
                                        data: serde_json::Value::Object(summary),
                                    }],
                                },
                                duration_ms,
                                attributes: attrs,
                            });
                        }
                    }
                    "snapshot" | "step-start" | "step-finish" => {
                        // Skip internal state markers
                    }
                    _ => {}
                }
            }
        } else {
            // Message without parts — emit as user message if user role
            if msg.role == "user" {
                let mut attrs = message_attrs.clone();
                attach_source_attrs(&mut attrs, Some(schema_version), Some("message:no-parts"));
                events.push(Event {
                    event_id: msg.id.clone(),
                    timestamp: msg_ts,
                    event_type: EventType::UserMessage,
                    task_id: None,
                    content: Content::empty(),
                    duration_ms: None,
                    attributes: attrs,
                });
            }
        }
    }

    for (task_id, (ts, origin_part_id)) in open_tasks {
        let mut attrs = HashMap::new();
        attach_source_attrs(&mut attrs, Some(schema_version), Some("synthetic:task-end"));
        attach_semantic_attrs(&mut attrs, Some(&task_id), None, Some("task"));
        events.push(Event {
            event_id: format!("{origin_part_id}-task-end-eof"),
            timestamp: ts,
            event_type: EventType::TaskEnd {
                summary: Some("closed at EOF".to_string()),
            },
            task_id: Some(task_id),
            content: Content::empty(),
            duration_ms: None,
            attributes: attrs,
        });
    }

    let created_at = info
        .time
        .as_ref()
        .and_then(|t| t.created)
        .map(millis_to_datetime)
        .or_else(|| events.first().map(|e| e.timestamp))
        .unwrap_or_else(Utc::now);
    let updated_at = info
        .time
        .as_ref()
        .and_then(|t| t.updated)
        .map(millis_to_datetime)
        .or_else(|| events.last().map(|e| e.timestamp))
        .unwrap_or_else(Utc::now);

    let agent = Agent {
        provider: provider_id.unwrap_or_else(|| "unknown".to_string()),
        model: model_id.unwrap_or_else(|| "unknown".to_string()),
        tool: "opencode".to_string(),
        tool_version: info.version.clone(),
    };

    let mut attributes = HashMap::new();
    if let Some(ref dir) = info.directory {
        attributes.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
    }
    attributes.insert(
        "source_path".to_string(),
        serde_json::Value::String(info_path.to_string_lossy().to_string()),
    );

    let mut related_session_ids = Vec::new();
    let mut session_role = "primary";
    if let Some(parent_id) = info.parent_id.as_ref() {
        let trimmed = parent_id.trim();
        if !trimmed.is_empty() && trimmed != info.id {
            related_session_ids.push(trimmed.to_string());
            attributes.insert(
                "parent_session_id".to_string(),
                serde_json::Value::String(trimmed.to_string()),
            );
            session_role = "auxiliary";
        }
    }
    attributes.insert(
        "session_role".to_string(),
        serde_json::Value::String(session_role.to_string()),
    );

    let context = SessionContext {
        title: info.title,
        description: None,
        tags: vec!["opencode".to_string()],
        created_at,
        updated_at,
        related_session_ids,
        attributes,
    };

    let mut session = Session::new(info.id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

fn millis_to_datetime(ms: u64) -> DateTime<Utc> {
    let secs = (ms / 1000) as i64;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nsecs)
        .single()
        .unwrap_or_else(Utc::now)
}

fn resolve_part_dir_for_message(
    part_base_dir: &Path,
    message_id: &str,
) -> Option<std::path::PathBuf> {
    let direct = part_base_dir.join(message_id);
    if direct.exists() {
        return Some(direct);
    }

    if message_id.starts_with("msg_") {
        let trimmed = message_id.trim_start_matches("msg_");
        let trimmed_path = part_base_dir.join(trimmed);
        if trimmed_path.exists() {
            return Some(trimmed_path);
        }
    } else {
        let prefixed = part_base_dir.join(format!("msg_{message_id}"));
        if prefixed.exists() {
            return Some(prefixed);
        }
    }
    None
}

fn classify_opencode_tool(name: &str, input: &Option<serde_json::Value>) -> EventType {
    let input = input.as_ref();
    match name {
        "bash" | "shell" => {
            let cmd = input
                .and_then(|v| json_find_string(v, &["command", "cmd", "script"]))
                .unwrap_or_default();
            EventType::ShellCommand {
                command: cmd,
                exit_code: None,
            }
        }
        "edit" | "str_replace_editor" => {
            let path = input
                .and_then(json_find_path)
                .unwrap_or_else(|| "unknown".to_string());
            EventType::FileEdit { path, diff: None }
        }
        "write" | "create" => {
            let path = input
                .and_then(json_find_path)
                .unwrap_or_else(|| "unknown".to_string());
            EventType::FileCreate { path }
        }
        "read" | "view" => {
            let path = input
                .and_then(json_find_path)
                .unwrap_or_else(|| "unknown".to_string());
            EventType::FileRead { path }
        }
        "grep" | "search" => {
            let query = input
                .and_then(|v| json_find_string(v, &["pattern", "query", "text", "regex"]))
                .unwrap_or_default();
            EventType::CodeSearch { query }
        }
        "glob" | "find" => {
            let pattern = input
                .and_then(|v| json_find_string(v, &["pattern", "path", "glob"]))
                .unwrap_or_else(|| "*".to_string());
            EventType::FileSearch { pattern }
        }
        "webfetch" | "web_fetch" => {
            let url = input
                .and_then(|v| json_find_string(v, &["url", "link"]))
                .unwrap_or_default();
            EventType::WebFetch { url }
        }
        "websearch" | "web_search" => {
            let query = input
                .and_then(|v| json_find_string(v, &["query", "q", "text"]))
                .unwrap_or_default();
            EventType::WebSearch { query }
        }
        "task" => EventType::ToolCall {
            name: "task".to_string(),
        },
        _ => EventType::ToolCall {
            name: name.to_string(),
        },
    }
}

fn opencode_tool_content(name: &str, input: &Option<serde_json::Value>) -> Content {
    let input = input.as_ref();
    match name {
        "bash" | "shell" => {
            let cmd = input
                .and_then(|v| v.get("command").and_then(|c| c.as_str()))
                .unwrap_or("");
            Content::code(cmd, Some("bash".to_string()))
        }
        _ => {
            if let Some(v) = input {
                Content {
                    blocks: vec![opensession_core::trace::ContentBlock::Json { data: v.clone() }],
                }
            } else {
                Content::empty()
            }
        }
    }
}

fn is_terminal_tool_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "complete" | "done" | "error" | "failed" | "cancelled" | "canceled"
    )
}

fn is_result_tool_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "complete" | "done" | "error" | "failed"
    )
}

fn normalized_call_id(call_id: Option<&str>) -> Option<String> {
    call_id
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn value_to_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        other => Some(other.to_string()),
    }
}

fn extract_tool_output_text(state: Option<&ToolState>) -> Option<String> {
    let state = state?;
    if let Some(output) = state.output.as_ref().and_then(value_to_text) {
        return Some(output);
    }
    if let Some(error) = state.error.as_ref().and_then(value_to_text) {
        return Some(error);
    }
    state
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("output"))
        .and_then(value_to_text)
}

fn reasoning_has_encrypted_payload(metadata: Option<&serde_json::Value>) -> bool {
    metadata
        .and_then(|meta| meta.get("openai"))
        .and_then(|openai| openai.get("reasoningEncryptedContent"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn json_find_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    if let serde_json::Value::Object(map) = value {
        for key in keys {
            if let Some(found) = map.get(*key).and_then(|v| v.as_str()) {
                if !found.trim().is_empty() {
                    return Some(found.to_string());
                }
            }
        }
        for nested in map.values() {
            if let Some(found) = json_find_string(nested, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn json_find_path(value: &serde_json::Value) -> Option<String> {
    const PATH_KEYS: &[&str] = &[
        "path",
        "file_path",
        "filePath",
        "filepath",
        "target_file",
        "targetFile",
        "target_path",
        "targetPath",
        "file",
        "filename",
    ];
    if let Some(path) = json_find_string(value, PATH_KEYS) {
        return Some(path);
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                let key_lower = key.to_ascii_lowercase();
                if (key_lower.contains("path") || key_lower == "file" || key_lower == "filename")
                    && nested.as_str().is_some()
                {
                    if let Some(raw) = nested.as_str() {
                        if !raw.trim().is_empty() {
                            return Some(raw.to_string());
                        }
                    }
                }
                if let Some(path) = json_find_path(nested) {
                    return Some(path);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_millis_to_datetime() {
        let dt = millis_to_datetime(1753359830903);
        assert!(dt.year() >= 2025);
    }

    #[test]
    fn test_classify_bash() {
        let input = Some(serde_json::json!({"command": "ls -la"}));
        let et = classify_opencode_tool("bash", &input);
        match et {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "ls -la"),
            _ => panic!("Expected ShellCommand"),
        }
    }

    #[test]
    fn test_classify_read_with_camel_case_path() {
        let input = Some(serde_json::json!({"filePath": "/tmp/demo.rs"}));
        let et = classify_opencode_tool("read", &input);
        match et {
            EventType::FileRead { path } => assert_eq!(path, "/tmp/demo.rs"),
            _ => panic!("Expected FileRead"),
        }
    }

    #[test]
    fn test_tool_status_terminal_variants() {
        assert!(is_terminal_tool_status("completed"));
        assert!(is_terminal_tool_status("FAILED"));
        assert!(is_terminal_tool_status("canceled"));
        assert!(!is_terminal_tool_status("running"));
    }

    #[test]
    fn test_normalized_call_id_trims_whitespace() {
        assert_eq!(
            normalized_call_id(Some("  functions.edit:27 ")).as_deref(),
            Some("functions.edit:27")
        );
        assert_eq!(normalized_call_id(Some("   ")), None);
    }

    #[test]
    fn test_extract_tool_output_text_fallbacks() {
        let state_output = ToolState {
            status: Some("completed".to_string()),
            input: None,
            output: Some(serde_json::json!("done")),
            error: Some(serde_json::json!("ignored error")),
            metadata: None,
            title: None,
            time: None,
        };
        assert_eq!(
            extract_tool_output_text(Some(&state_output)).as_deref(),
            Some("done")
        );

        let state_error = ToolState {
            status: Some("error".to_string()),
            input: None,
            output: None,
            error: Some(serde_json::json!("failed")),
            metadata: Some(serde_json::json!({"output": "metadata output"})),
            title: None,
            time: None,
        };
        assert_eq!(
            extract_tool_output_text(Some(&state_error)).as_deref(),
            Some("failed")
        );

        let state_meta = ToolState {
            status: Some("error".to_string()),
            input: None,
            output: None,
            error: None,
            metadata: Some(serde_json::json!({"output": "metadata output"})),
            title: None,
            time: None,
        };
        assert_eq!(
            extract_tool_output_text(Some(&state_meta)).as_deref(),
            Some("metadata output")
        );
    }

    #[test]
    fn test_session_info_deser() {
        let json = r#"{"id":"ses_abc","version":"1.1.30","title":"Test session","projectID":"abc123","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "ses_abc");
        assert_eq!(info.title, Some("Test session".to_string()));
        assert_eq!(info.directory, Some("/tmp/proj".to_string()));
    }

    #[test]
    fn test_session_info_parent_id_deser() {
        let json = r#"{"id":"ses_child","version":"1.1.30","parentID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.parent_id, Some("ses_parent".to_string()));
    }

    #[test]
    fn test_session_info_parent_id_alias_deser() {
        let json = r#"{"id":"ses_child","version":"1.1.30","parentId":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.parent_id, Some("ses_parent".to_string()));
    }

    #[test]
    fn test_session_info_parent_uuid_alias_deser() {
        let json = r#"{"id":"ses_child","version":"1.1.30","parentUUID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.parent_id, Some("ses_parent".to_string()));
    }

    #[test]
    fn test_session_context_has_source_path() {
        let temp_dir = std::env::temp_dir().join(format!(
            "opensession-opencode-parser-source-path-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock ok")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let session_path = temp_dir.join("session.json");
        write(
            &session_path,
            r#"{"id":"ses_parent","version":"1.1.30","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
        )
        .unwrap();

        let session = parse_opencode_session(&session_path).expect("parse session");
        assert_eq!(
            session
                .context
                .attributes
                .get("source_path")
                .and_then(|value| value.as_str()),
            Some(session_path.to_str().unwrap())
        );
    }

    #[test]
    fn test_message_info_deser() {
        let json = r#"{"id":"msg_abc","sessionID":"ses_abc","role":"user","model":{"providerID":"openai","modelID":"gpt-5.2-codex"},"time":{"created":1753359830903}}"#;
        let msg: MessageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(msg.id, "msg_abc");
        assert_eq!(msg.role, "user");
        let model = msg.model.unwrap();
        assert_eq!(model.provider_id, Some("openai".to_string()));
        assert_eq!(model.model_id, Some("gpt-5.2-codex".to_string()));
    }

    #[test]
    fn test_message_info_deser_top_level_model_fields() {
        let json = r#"{"id":"msg_xyz","sessionID":"ses_abc","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359830903}}"#;
        let msg: MessageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(msg.id, "msg_xyz");
        assert_eq!(msg.provider_id.as_deref(), Some("openai"));
        assert_eq!(msg.model_id.as_deref(), Some("gpt-5.2-codex"));
    }

    #[test]
    fn test_can_parse() {
        let parser = OpenCodeParser;
        assert!(parser.can_parse(Path::new(
            "/Users/test/.local/share/opencode/storage/session/abc123/ses_xyz.json"
        )));
        assert!(!parser.can_parse(Path::new(
            "/Users/test/.local/share/opencode/storage/message/ses_xyz/msg_abc.json"
        )));
    }

    fn tmp_test_root() -> std::path::PathBuf {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("opensession-opencode-parser-{since_epoch}"));
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    #[test]
    fn test_parse_relates_child_session_to_parent() {
        let root = tmp_test_root();
        let project = root.join("proj-test");
        let session_dir = project.join("storage").join("session").join("example");
        let message_dir = project.join("storage").join("message").join("ses_child");
        let part_dir = project.join("storage").join("part").join("msg_001");
        create_dir_all(&session_dir).expect("create session dir");
        create_dir_all(&message_dir).expect("create message dir");
        create_dir_all(&part_dir).expect("create part dir");

        write(
            session_dir.join("ses_child.json"),
            r#"{"id":"ses_child","version":"1.1.30","parentID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
        )
        .expect("write session file");
        write(
            message_dir.join("msg_001.json"),
            r#"{"id":"msg_001","sessionID":"ses_child","role":"user","time":{"created":1753359831000}}"#,
        )
        .expect("write message file");
        write(
            part_dir.join("part_001.json"),
            r#"{"id":"part_001","messageID":"msg_001","type":"text","text":"hello","time":{"start":1753359831000,"end":1753359831000}}"#,
        )
        .expect("write part file");

        let session =
            parse_opencode_session(&session_dir.join("ses_child.json")).expect("parse session");
        assert_eq!(
            session.context.related_session_ids,
            vec!["ses_parent".to_string()]
        );
        assert_eq!(
            session
                .context
                .attributes
                .get("parent_session_id")
                .and_then(|v| v.as_str()),
            Some("ses_parent")
        );
        assert_eq!(
            session
                .context
                .attributes
                .get("session_role")
                .and_then(|v| v.as_str()),
            Some("auxiliary")
        );
        assert_eq!(session.stats.event_count, 1);
    }

    #[test]
    fn test_parse_part_dir_prefixed_msg_fallback() {
        let root = tmp_test_root();
        let project = root.join("proj-prefixed");
        let session_dir = project.join("storage").join("session").join("example");
        let message_dir = project.join("storage").join("message").join("ses_fallback");
        let part_dir = project.join("storage").join("part").join("msg_abc123");
        create_dir_all(&session_dir).expect("create session dir");
        create_dir_all(&message_dir).expect("create message dir");
        create_dir_all(&part_dir).expect("create part dir");

        write(
            session_dir.join("ses_fallback.json"),
            r#"{"id":"ses_fallback","version":"1.1.30","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
        )
        .expect("write session file");
        write(
            message_dir.join("abc123.json"),
            r#"{"id":"abc123","sessionID":"ses_fallback","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359831000}}"#,
        )
        .expect("write message file");
        write(
            part_dir.join("part_001.json"),
            r#"{"id":"part_001","messageID":"abc123","type":"text","text":"assistant reply","time":{"start":1753359831000,"end":1753359831200}}"#,
        )
        .expect("write part file");

        let session =
            parse_opencode_session(&session_dir.join("ses_fallback.json")).expect("parse session");
        assert!(session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::AgentMessage)));
        assert_eq!(session.agent.provider, "openai");
        assert_eq!(session.agent.model, "gpt-5.2-codex");
        assert_eq!(
            session
                .context
                .attributes
                .get("session_role")
                .and_then(|v| v.as_str()),
            Some("primary")
        );
    }

    #[test]
    fn test_parse_reasoning_and_call_id_normalization() {
        let root = tmp_test_root();
        let project = root.join("proj-company");
        let session_dir = project.join("storage").join("session").join("example");
        let message_dir = project.join("storage").join("message").join("ses_company");
        let user_part_dir = project.join("storage").join("part").join("msg_user");
        let assistant_part_dir = project.join("storage").join("part").join("msg_assistant");
        create_dir_all(&session_dir).expect("create session dir");
        create_dir_all(&message_dir).expect("create message dir");
        create_dir_all(&user_part_dir).expect("create user part dir");
        create_dir_all(&assistant_part_dir).expect("create assistant part dir");

        write(
            session_dir.join("ses_company.json"),
            r#"{"id":"ses_company","version":"1.2.0","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
        )
        .expect("write session");
        write(
            message_dir.join("msg_user.json"),
            r#"{"id":"msg_user","sessionID":"ses_company","role":"user","model":{"providerID":"openai","modelID":"gpt-5.2-codex"},"time":{"created":1753359831000}}"#,
        )
        .expect("write user message");
        write(
            message_dir.join("msg_assistant.json"),
            r#"{"id":"msg_assistant","sessionID":"ses_company","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359832000,"completed":1753359835000}}"#,
        )
        .expect("write assistant message");

        write(
            user_part_dir.join("part_user_text.json"),
            r#"{"id":"part_user_text","messageID":"msg_user","type":"text","text":"run diagnostics","time":{"start":1753359831000,"end":1753359831100}}"#,
        )
        .expect("write user text part");
        write(
            user_part_dir.join("part_user_file.json"),
            r#"{"id":"part_user_file","messageID":"msg_user","type":"file","filename":"notes.md","url":"file:///tmp/proj/notes.md","time":{"start":1753359831050,"end":1753359831050}}"#,
        )
        .expect("write user file part");
        write(
            assistant_part_dir.join("part_reasoning.json"),
            r#"{"id":"part_reasoning","messageID":"msg_assistant","type":"reasoning","text":"","metadata":{"openai":{"reasoningEncryptedContent":"abc"}},"time":{"start":1753359832100,"end":1753359832200}}"#,
        )
        .expect("write reasoning part");
        write(
            assistant_part_dir.join("part_tool_done.json"),
            r#"{"id":"part_tool_done","messageID":"msg_assistant","type":"tool","callID":"call_abc","tool":"grep","state":{"status":"Completed","input":{"pattern":"todo","path":"/tmp/proj"},"output":"Found 1 match","title":"todo","time":{"start":1753359832300,"end":1753359832400}}}"#,
        )
        .expect("write completed tool part");
        write(
            assistant_part_dir.join("part_tool_running.json"),
            r#"{"id":"part_tool_running","messageID":"msg_assistant","type":"tool","callID":"  functions.edit:27 ","tool":"edit","state":{"status":"running","input":{"filePath":"/tmp/proj/main.rs"},"time":{"start":1753359832500}}}"#,
        )
        .expect("write running tool part");
        write(
            assistant_part_dir.join("part_patch.json"),
            r#"{"id":"part_patch","messageID":"msg_assistant","type":"patch","hash":"abc123","files":["/tmp/proj/main.rs","/tmp/proj/lib.rs"],"time":{"start":1753359832450,"end":1753359832450}}"#,
        )
        .expect("write patch part");

        let session =
            parse_opencode_session(&session_dir.join("ses_company.json")).expect("parse session");

        let thinking = session
            .events
            .iter()
            .find(|event| matches!(event.event_type, EventType::Thinking))
            .expect("thinking event");
        assert_eq!(
            thinking
                .content
                .blocks
                .first()
                .and_then(|block| match block {
                    opensession_core::trace::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                }),
            Some("Encrypted reasoning")
        );

        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::ToolResult { name, call_id, .. }
                    if name == "grep" && call_id.as_deref() == Some("call_abc")
            )
        }));

        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && event.content.blocks.iter().any(|block| {
                    matches!(
                        block,
                        opensession_core::trace::ContentBlock::Text { text }
                            if text == "Attached file: notes.md"
                    )
                })
        }));

        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::FileEdit { path, .. } if path == "/tmp/proj/lib.rs"
            ) && event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("part:patch:file")
        }));

        assert!(session.events.iter().any(|event| {
            matches!(&event.event_type, EventType::FileEdit { .. })
                && event
                    .attributes
                    .get("semantic.call_id")
                    .and_then(|v| v.as_str())
                    == Some("functions.edit:27")
        }));

        assert!(session.events.iter().any(|event| {
            matches!(&event.event_type, EventType::TaskEnd { .. })
                && event.task_id.as_deref() == Some("functions.edit:27")
                && event
                    .attributes
                    .get("source.raw_type")
                    .and_then(|v| v.as_str())
                    == Some("synthetic:task-end")
        }));
    }

    #[test]
    fn test_patch_with_many_files_emits_summary_event() {
        let root = tmp_test_root();
        let project = root.join("proj-patch-summary");
        let session_dir = project.join("storage").join("session").join("example");
        let message_dir = project
            .join("storage")
            .join("message")
            .join("ses_patch_summary");
        let part_dir = project.join("storage").join("part").join("msg_assistant");
        create_dir_all(&session_dir).expect("create session dir");
        create_dir_all(&message_dir).expect("create message dir");
        create_dir_all(&part_dir).expect("create part dir");

        write(
            session_dir.join("ses_patch_summary.json"),
            r#"{"id":"ses_patch_summary","version":"1.2.0","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
        )
        .expect("write session");
        write(
            message_dir.join("msg_assistant.json"),
            r#"{"id":"msg_assistant","sessionID":"ses_patch_summary","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359832000,"completed":1753359835000}}"#,
        )
        .expect("write assistant message");
        write(
            part_dir.join("part_patch_many.json"),
            r#"{"id":"part_patch_many","messageID":"msg_assistant","type":"patch","hash":"manyhash","files":["/tmp/proj/f1.rs","/tmp/proj/f2.rs","/tmp/proj/f3.rs","/tmp/proj/f4.rs","/tmp/proj/f5.rs","/tmp/proj/f6.rs","/tmp/proj/f7.rs","/tmp/proj/f8.rs","/tmp/proj/f9.rs"],"time":{"start":1753359832100,"end":1753359832200}}"#,
        )
        .expect("write patch part");

        let session = parse_opencode_session(&session_dir.join("ses_patch_summary.json"))
            .expect("parse patch summary session");

        assert!(!session
            .events
            .iter()
            .any(|event| matches!(&event.event_type, EventType::FileEdit { .. })));
        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::Custom { kind } if kind == "patch"
            ) && event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("part:patch:summary")
                && event.content.blocks.iter().any(|block| {
                    matches!(
                        block,
                        opensession_core::trace::ContentBlock::Json { data }
                            if data
                                .get("file_count")
                                .and_then(|v| v.as_u64())
                                == Some(9)
                    )
                })
        }));
    }

    use chrono::Datelike;
}
