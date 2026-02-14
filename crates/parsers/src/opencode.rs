use crate::common::set_first;
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
    // tool parts
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    state: Option<ToolState>,
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
    output: Option<String>,
    #[serde(default)]
    error: Option<String>,
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
        let msg_part_dir = part_base_dir.join(&msg.id);
        if !msg_part_dir.exists() {
            continue;
        }
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

    for msg in &messages {
        // Extract model from nested model object
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
                        events.push(Event {
                            event_id: part.id.clone(),
                            timestamp: part_ts,
                            event_type,
                            task_id: None,
                            content: Content::text(text),
                            duration_ms,
                            attributes: HashMap::new(),
                        });
                    }
                    "tool" => {
                        let tool_name = part.tool.as_deref().unwrap_or("unknown").to_string();
                        let state = part.state.as_ref();
                        let status = state.and_then(|s| s.status.as_deref()).unwrap_or("unknown");
                        let task_id = part
                            .call_id
                            .as_deref()
                            .filter(|id| !id.trim().is_empty())
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| format!("opencode-task-{}", part.id));
                        let title = state
                            .and_then(|s| s.title.as_deref())
                            .filter(|v| !v.trim().is_empty())
                            .map(str::to_string)
                            .or_else(|| Some(tool_name.clone()));

                        events.push(Event {
                            event_id: format!("{}-task-start", part.id),
                            timestamp: part_ts,
                            event_type: EventType::TaskStart { title },
                            task_id: Some(task_id.clone()),
                            content: Content::empty(),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                        open_tasks.insert(task_id.clone(), (part_ts, part.id.clone()));

                        // Emit ToolCall
                        let input = state.and_then(|s| s.input.clone());
                        let event_type = classify_opencode_tool(&tool_name, &input);
                        let content = opencode_tool_content(&tool_name, &input);

                        events.push(Event {
                            event_id: format!("{}-call", part.id),
                            timestamp: part_ts,
                            event_type,
                            task_id: Some(task_id.clone()),
                            content,
                            duration_ms,
                            attributes: HashMap::new(),
                        });

                        // Emit ToolResult if completed or error
                        let call_event_id = format!("{}-call", part.id);
                        if status == "completed" || status == "error" {
                            let output_text = state
                                .and_then(|s| s.output.as_deref())
                                .or_else(|| state.and_then(|s| s.error.as_deref()))
                                .unwrap_or("")
                                .to_string();

                            events.push(Event {
                                event_id: format!("{}-result", part.id),
                                timestamp: part_ts,
                                event_type: EventType::ToolResult {
                                    name: tool_name.clone(),
                                    is_error: status == "error",
                                    call_id: Some(call_event_id),
                                },
                                task_id: Some(task_id.clone()),
                                content: Content::text(&output_text),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }

                        if is_terminal_tool_status(status) {
                            let summary = state
                                .and_then(|s| s.title.as_deref())
                                .filter(|v| !v.trim().is_empty())
                                .map(str::to_string)
                                .or_else(|| {
                                    if status == "error" {
                                        Some(format!("{tool_name} failed"))
                                    } else {
                                        Some(format!("{tool_name} {status}"))
                                    }
                                });
                            events.push(Event {
                                event_id: format!("{}-task-end", part.id),
                                timestamp: part_ts,
                                event_type: EventType::TaskEnd { summary },
                                task_id: Some(task_id.clone()),
                                content: Content::empty(),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                            open_tasks.remove(&task_id);
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
                events.push(Event {
                    event_id: msg.id.clone(),
                    timestamp: msg_ts,
                    event_type: EventType::UserMessage,
                    task_id: None,
                    content: Content::empty(),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
        }
    }

    for (task_id, (ts, origin_part_id)) in open_tasks {
        events.push(Event {
            event_id: format!("{origin_part_id}-task-end-eof"),
            timestamp: ts,
            event_type: EventType::TaskEnd {
                summary: Some("closed at EOF".to_string()),
            },
            task_id: Some(task_id),
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
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

    let context = SessionContext {
        title: info.title,
        description: None,
        tags: vec!["opencode".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
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
    fn test_session_info_deser() {
        let json = r#"{"id":"ses_abc","version":"1.1.30","title":"Test session","projectID":"abc123","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "ses_abc");
        assert_eq!(info.title, Some("Test session".to_string()));
        assert_eq!(info.directory, Some("/tmp/proj".to_string()));
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
    fn test_can_parse() {
        let parser = OpenCodeParser;
        assert!(parser.can_parse(Path::new(
            "/Users/test/.local/share/opencode/storage/session/abc123/ses_xyz.json"
        )));
        assert!(!parser.can_parse(Path::new(
            "/Users/test/.local/share/opencode/storage/message/ses_xyz/msg_abc.json"
        )));
    }

    use chrono::Datelike;
}
