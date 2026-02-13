use crate::common::{
    build_tool_result_content, extract_tag_content, set_first, strip_system_reminders, ToolUseInfo,
};
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

pub struct ClineParser;

impl SessionParser for ClineParser {
    fn name(&self) -> &str {
        "cline"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.file_name()
            .is_some_and(|f| f == "api_conversation_history.json")
            && path.to_str().is_some_and(|s| {
                s.contains(".cline/data/tasks/") || s.contains("cline/data/tasks/")
            })
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_cline_task(path)
    }
}

// ── Raw deserialization types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ApiMessage {
    role: String,
    content: Vec<ApiContentBlock>,
    #[serde(default, rename = "modelInfo")]
    model_info: Option<ModelInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum ApiContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
        #[serde(default)]
        signature: Option<String>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default, rename = "tool_use_id")]
        tool_use_id: Option<String>,
        #[serde(default)]
        content: Option<serde_json::Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ModelInfo {
    #[serde(default, rename = "providerId")]
    provider_id: Option<String>,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct UiMessage {
    ts: u64,
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    say: Option<String>,
    #[serde(default)]
    ask: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "modelInfo")]
    model_info: Option<ModelInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TaskHistoryEntry {
    id: String,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    ts: u64,
    #[serde(default, rename = "tokensIn")]
    tokens_in: u64,
    #[serde(default, rename = "tokensOut")]
    tokens_out: u64,
    #[serde(default, rename = "totalCost")]
    total_cost: f64,
    #[serde(default, rename = "cwdOnTaskInitialization")]
    cwd: Option<String>,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
}

// ── Cline-specific text patterns ────────────────────────────────────────────

/// Matches tool result text: `[tool_name for 'arg'] Result:` or `[tool_name] Result:`
static TOOL_RESULT_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[(\w+)(?:\s+for\s+'([^']*)')?\]\s+Result:\n?").unwrap());

/// Parse Cline's tool result text format.
/// Returns (tool_name, file_path_arg, result_text) if it matches.
fn parse_tool_result_text(text: &str) -> Option<(String, Option<String>, String)> {
    let caps = TOOL_RESULT_PREFIX_RE.captures(text)?;
    let tool_name = caps[1].to_string();
    let file_path = caps.get(2).map(|m| m.as_str().to_string());
    let prefix_end = caps.get(0)?.end();
    let result_text = text[prefix_end..].to_string();
    Some((tool_name, file_path, result_text))
}

// ── Parsing logic ───────────────────────────────────────────────────────────

fn parse_cline_task(api_history_path: &Path) -> Result<Session> {
    let task_dir = api_history_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid path"))?;
    let task_id = task_dir
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown")
        .to_string();

    let api_text = std::fs::read_to_string(api_history_path)
        .with_context(|| format!("Failed to read: {}", api_history_path.display()))?;
    let api_messages: Vec<ApiMessage> = serde_json::from_str(&api_text)
        .with_context(|| "Failed to parse api_conversation_history.json")?;

    // Read UI messages for timestamps
    let ui_path = task_dir.join("ui_messages.json");
    let ui_messages: Vec<UiMessage> = if ui_path.exists() {
        let ui_text = std::fs::read_to_string(&ui_path).unwrap_or_default();
        serde_json::from_str(&ui_text).unwrap_or_default()
    } else {
        Vec::new()
    };

    let task_title = find_task_title(task_dir, &task_id);
    let first_ts = ui_messages.first().map(|m| m.ts);
    let last_ts = ui_messages.last().map(|m| m.ts);

    // Detect model/provider (first-wins from api_messages, then fallback to ui_messages)
    let mut model_id: Option<String> = None;
    let mut provider_id: Option<String> = None;
    for info in api_messages
        .iter()
        .filter_map(|msg| msg.model_info.as_ref())
        .chain(ui_messages.iter().filter_map(|msg| msg.model_info.as_ref()))
    {
        set_first(&mut model_id, info.model_id.clone());
        set_first(&mut provider_id, info.provider_id.clone());
        if model_id.is_some() && provider_id.is_some() {
            break;
        }
    }

    // Convert to HAIL events
    let mut events: Vec<Event> = Vec::new();
    let mut event_counter = 0u64;
    let base_ts = first_ts.unwrap_or(
        task_id
            .parse::<u64>()
            .unwrap_or(Utc::now().timestamp_millis() as u64),
    );

    // Track tool_use info for ToolResult pairing
    let mut tool_use_map: HashMap<String, ToolUseInfo> = HashMap::new();
    let mut last_tool_info = ToolUseInfo {
        name: "unknown".to_string(),
        file_path: None,
    };

    for (msg_idx, msg) in api_messages.iter().enumerate() {
        let msg_ts = millis_to_datetime(base_ts + (msg_idx as u64) * 100);

        match msg.role.as_str() {
            "user" => process_user_msg(
                msg,
                msg_ts,
                &mut events,
                &mut event_counter,
                &tool_use_map,
                &last_tool_info,
            ),
            "assistant" => process_assistant_msg(
                msg,
                msg_ts,
                &mut events,
                &mut event_counter,
                &mut tool_use_map,
                &mut last_tool_info,
            ),
            _ => {}
        }
    }

    let created_at = first_ts.map(millis_to_datetime).unwrap_or_else(Utc::now);
    let updated_at = last_ts.map(millis_to_datetime).unwrap_or(created_at);

    let agent = Agent {
        provider: provider_id.unwrap_or_else(|| "unknown".to_string()),
        model: model_id.unwrap_or_else(|| "unknown".to_string()),
        tool: "cline".to_string(),
        tool_version: None,
    };

    let context = SessionContext {
        title: task_title,
        description: None,
        tags: vec!["cline".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes: HashMap::new(),
    };

    let mut session = Session::new(task_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

/// Process a user-role message in Cline's API format.
///
/// In Cline, user messages contain:
/// - `<task>prompt</task>` — the user's actual task/prompt
/// - `<environment_details>...` — system context (skip)
/// - `# task_progress ...` — system bookkeeping (skip)
/// - `[tool_name for 'path'] Result:\n...` — tool result as text
/// - `<user_message>text</user_message>` — user's inline response (inside plan_mode_respond results)
/// - `tool_result` blocks — formal tool results
fn process_user_msg(
    msg: &ApiMessage,
    ts: chrono::DateTime<Utc>,
    events: &mut Vec<Event>,
    counter: &mut u64,
    tool_use_map: &HashMap<String, ToolUseInfo>,
    last_tool_info: &ToolUseInfo,
) {
    for block in &msg.content {
        match block {
            ApiContentBlock::Text { text } => {
                if text.is_empty() {
                    continue;
                }

                // Skip Cline system noise
                if text.starts_with("<environment_details>") {
                    continue;
                }
                if text.contains("# task_progress") {
                    continue;
                }

                // 1. User task prompt: <task>prompt</task>
                if text.starts_with("<task>") {
                    if let Some(task_text) = extract_tag_content(text, "task") {
                        *counter += 1;
                        events.push(Event {
                            event_id: format!("cline-{}", counter),
                            timestamp: ts,
                            event_type: EventType::UserMessage,
                            task_id: None,
                            content: Content::text(task_text),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    continue;
                }

                // 2. Tool result text: [tool_name for 'path'] Result:\n...
                if let Some((tool_name, file_path, result_text)) = parse_tool_result_text(text) {
                    // Check for embedded <user_message> in plan_mode_respond results
                    if tool_name == "plan_mode_respond" || text.contains("<user_message>") {
                        if let Some(user_text) = extract_tag_content(text, "user_message") {
                            *counter += 1;
                            events.push(Event {
                                event_id: format!("cline-{}", counter),
                                timestamp: ts,
                                event_type: EventType::UserMessage,
                                task_id: None,
                                content: Content::text(user_text),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                        continue;
                    }

                    // Regular tool result — transform content like Read results into Code blocks
                    let info = ToolUseInfo {
                        name: tool_name.clone(),
                        file_path,
                    };
                    let content = build_tool_result_content(&result_text, &info);
                    *counter += 1;
                    events.push(Event {
                        event_id: format!("cline-{}", counter),
                        timestamp: ts,
                        event_type: EventType::ToolResult {
                            name: tool_name,
                            is_error: false,
                            call_id: None,
                        },
                        task_id: None,
                        content,
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                    continue;
                }

                // 3. Inline <user_message> (not part of a tool result prefix)
                if text.contains("<user_message>") {
                    if let Some(user_text) = extract_tag_content(text, "user_message") {
                        *counter += 1;
                        events.push(Event {
                            event_id: format!("cline-{}", counter),
                            timestamp: ts,
                            event_type: EventType::UserMessage,
                            task_id: None,
                            content: Content::text(user_text),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    continue;
                }

                // 4. Remaining text from user role: likely user message
                let cleaned = strip_system_reminders(text);
                if !cleaned.trim().is_empty() {
                    *counter += 1;
                    events.push(Event {
                        event_id: format!("cline-{}", counter),
                        timestamp: ts,
                        event_type: EventType::UserMessage,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }
            }
            ApiContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                let output = extract_tool_result_json(content);
                let info = tool_use_id
                    .as_ref()
                    .and_then(|id| tool_use_map.get(id))
                    .cloned()
                    .unwrap_or_else(|| last_tool_info.clone());
                let tool_name = info.name.clone();
                let result_content = build_tool_result_content(&output, &info);
                *counter += 1;
                events.push(Event {
                    event_id: format!("cline-{}", counter),
                    timestamp: ts,
                    event_type: EventType::ToolResult {
                        name: tool_name,
                        is_error: false,
                        call_id: tool_use_id.clone(),
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

/// Process an assistant-role message.
///
/// In Cline, assistant messages contain:
/// - `text` blocks — agent response text (rare, but used for inline responses)
/// - `thinking` blocks — reasoning
/// - `tool_use` blocks — tool calls
///   - `attempt_completion` → emit AgentMessage from `result` field
///   - `ask_followup_question` → emit AgentMessage from `question` field
fn process_assistant_msg(
    msg: &ApiMessage,
    ts: chrono::DateTime<Utc>,
    events: &mut Vec<Event>,
    counter: &mut u64,
    tool_use_map: &mut HashMap<String, ToolUseInfo>,
    last_tool_info: &mut ToolUseInfo,
) {
    for block in &msg.content {
        match block {
            ApiContentBlock::Text { text } => {
                if text.is_empty() {
                    continue;
                }
                let cleaned = strip_system_reminders(text);
                if !cleaned.is_empty() {
                    *counter += 1;
                    events.push(Event {
                        event_id: format!("cline-{}", counter),
                        timestamp: ts,
                        event_type: EventType::AgentMessage,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }
            }
            ApiContentBlock::Thinking { thinking, .. } => {
                if thinking.is_empty() {
                    continue;
                }
                *counter += 1;
                events.push(Event {
                    event_id: format!("cline-{}", counter),
                    timestamp: ts,
                    event_type: EventType::Thinking,
                    task_id: None,
                    content: Content::text(thinking),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
            ApiContentBlock::ToolUse {
                id, name, input, ..
            } => {
                // Extract file_path for ToolResult content transformation
                let file_path = match name.as_str() {
                    "read_file" => input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    "write_to_file" | "insert_content" | "apply_diff" | "replace_in_file"
                    | "search_and_replace" => input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    _ => None,
                };

                let info = ToolUseInfo {
                    name: name.clone(),
                    file_path,
                };

                // Track for ToolResult matching
                if let Some(tool_id) = id {
                    tool_use_map.insert(tool_id.clone(), info.clone());
                }
                *last_tool_info = info;

                // Special: attempt_completion → AgentMessage from result
                if name == "attempt_completion" {
                    if let Some(result) = input.get("result").and_then(|v| v.as_str()) {
                        if !result.is_empty() {
                            *counter += 1;
                            events.push(Event {
                                event_id: format!("cline-{}", counter),
                                timestamp: ts,
                                event_type: EventType::AgentMessage,
                                task_id: None,
                                content: Content::text(result),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    continue; // Don't also emit a ToolCall for attempt_completion
                }

                // Special: ask_followup_question → AgentMessage from question
                if name == "ask_followup_question" {
                    if let Some(q) = input.get("question").and_then(|v| v.as_str()) {
                        if !q.is_empty() {
                            *counter += 1;
                            events.push(Event {
                                event_id: format!("cline-{}", counter),
                                timestamp: ts,
                                event_type: EventType::AgentMessage,
                                task_id: None,
                                content: Content::text(q),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    continue;
                }

                // Special: plan_mode_respond → AgentMessage from response
                if name == "plan_mode_respond" {
                    if let Some(resp) = input.get("response").and_then(|v| v.as_str()) {
                        if !resp.is_empty() {
                            *counter += 1;
                            events.push(Event {
                                event_id: format!("cline-{}", counter),
                                timestamp: ts,
                                event_type: EventType::AgentMessage,
                                task_id: None,
                                content: Content::text(resp),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    continue;
                }

                // Regular tool call
                let event_type = classify_cline_tool(name, input);
                let content = cline_tool_content(name, input);
                *counter += 1;
                events.push(Event {
                    event_id: format!("cline-{}", counter),
                    timestamp: ts,
                    event_type,
                    task_id: None,
                    content,
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
            ApiContentBlock::ToolResult { .. } | ApiContentBlock::Unknown => {}
        }
    }
}

fn millis_to_datetime(ms: u64) -> chrono::DateTime<Utc> {
    let secs = (ms / 1000) as i64;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nsecs)
        .single()
        .unwrap_or_else(Utc::now)
}

fn find_task_title(task_dir: &Path, task_id: &str) -> Option<String> {
    let data_dir = task_dir
        .parent()? // tasks/
        .parent()?; // data/ or root

    // Try sibling: data/../state/taskHistory.json (common Cline layout)
    // Then try: data/state/taskHistory.json (alternative layout)
    let candidates = [
        data_dir
            .parent()
            .map(|p| p.join("state").join("taskHistory.json")),
        Some(data_dir.join("state").join("taskHistory.json")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if !candidate.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&candidate).ok()?;
        let entries: Vec<TaskHistoryEntry> = serde_json::from_str(&text).ok()?;
        if let Some(entry) = entries.iter().find(|e| e.id == task_id) {
            return entry.task.clone();
        }
    }

    None
}

fn classify_cline_tool(name: &str, input: &serde_json::Value) -> EventType {
    match name {
        "execute_command" | "spawn_process" => {
            let cmd = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::ShellCommand {
                command: cmd,
                exit_code: None,
            }
        }
        "write_to_file" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileCreate { path }
        }
        "insert_content" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "apply_diff" | "replace_in_file" | "search_and_replace" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "read_file" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        "search_files" | "find_references" => {
            let query = input
                .get("regex")
                .or_else(|| input.get("content_pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::CodeSearch { query }
        }
        "list_files" => {
            let pattern = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            EventType::FileSearch { pattern }
        }
        "list_code_definition_names" => EventType::ToolCall {
            name: name.to_string(),
        },
        "browser_action" | "use_mcp_tool" => EventType::ToolCall {
            name: name.to_string(),
        },
        _ => EventType::ToolCall {
            name: name.to_string(),
        },
    }
}

fn cline_tool_content(name: &str, input: &serde_json::Value) -> Content {
    match name {
        "execute_command" | "spawn_process" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            Content {
                blocks: vec![ContentBlock::Code {
                    code: cmd.to_string(),
                    language: Some("bash".to_string()),
                    start_line: None,
                }],
            }
        }
        "read_file" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "write_to_file" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "insert_content" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "apply_diff" | "replace_in_file" | "search_and_replace" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "search_files" | "find_references" => {
            let query = input
                .get("regex")
                .or_else(|| input.get("content_pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Content::text(query)
        }
        "list_files" => {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            Content::text(path)
        }
        _ => Content {
            blocks: vec![ContentBlock::Json {
                data: input.clone(),
            }],
        },
    }
}

/// Extract text from Cline's tool_result JSON content field
fn extract_tool_result_json(content: &Option<serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| {
                v.get("text")
                    .and_then(|t| t.as_str())
                    .map(|text| text.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_execute_command() {
        let input = serde_json::json!({"command": "npm run build"});
        let et = classify_cline_tool("execute_command", &input);
        match et {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "npm run build"),
            _ => panic!("Expected ShellCommand"),
        }
    }

    #[test]
    fn test_classify_write_to_file() {
        let input = serde_json::json!({"path": "src/index.ts", "content": "hello"});
        let et = classify_cline_tool("write_to_file", &input);
        match et {
            EventType::FileCreate { path } => assert_eq!(path, "src/index.ts"),
            _ => panic!("Expected FileCreate"),
        }
    }

    #[test]
    fn test_classify_read_file() {
        let input = serde_json::json!({"path": "src/main.rs"});
        let et = classify_cline_tool("read_file", &input);
        match et {
            EventType::FileRead { path } => assert_eq!(path, "src/main.rs"),
            _ => panic!("Expected FileRead"),
        }
    }

    #[test]
    fn test_classify_search_files() {
        let input = serde_json::json!({"regex": "fn main", "path": "."});
        let et = classify_cline_tool("search_files", &input);
        match et {
            EventType::CodeSearch { query } => assert_eq!(query, "fn main"),
            _ => panic!("Expected CodeSearch"),
        }
    }

    #[test]
    fn test_classify_list_files() {
        let input = serde_json::json!({"path": "src/"});
        let et = classify_cline_tool("list_files", &input);
        match et {
            EventType::FileSearch { pattern } => assert_eq!(pattern, "src/"),
            _ => panic!("Expected FileSearch"),
        }
    }

    #[test]
    fn test_extract_tool_result_json() {
        let content = Some(serde_json::json!("file contents here"));
        assert_eq!(extract_tool_result_json(&content), "file contents here");

        let content = Some(serde_json::json!([{"type": "text", "text": "result"}]));
        assert_eq!(extract_tool_result_json(&content), "result");
    }

    #[test]
    fn test_parse_tool_result_text_with_path() {
        let text = "[read_file for 'src/main.rs'] Result:\nfn main() {}";
        let (name, path, result) = parse_tool_result_text(text).unwrap();
        assert_eq!(name, "read_file");
        assert_eq!(path, Some("src/main.rs".to_string()));
        assert_eq!(result, "fn main() {}");
    }

    #[test]
    fn test_parse_tool_result_text_without_path() {
        let text = "[plan_mode_respond] Result:\n<user_message>hello</user_message>";
        let (name, path, result) = parse_tool_result_text(text).unwrap();
        assert_eq!(name, "plan_mode_respond");
        assert_eq!(path, None);
        assert!(result.contains("<user_message>"));
    }

    #[test]
    fn test_parse_tool_result_text_no_match() {
        let text = "This is just normal text";
        assert!(parse_tool_result_text(text).is_none());
    }

    #[test]
    fn test_task_tag_extraction() {
        let text = "<task>\nFix the authentication bug\n</task>";
        assert_eq!(
            extract_tag_content(text, "task"),
            Some("Fix the authentication bug".to_string())
        );
    }

    #[test]
    fn test_user_message_extraction() {
        let text = "[plan_mode_respond] Result:\n<user_message>\nhello world\n</user_message>";
        let (name, _, result) = parse_tool_result_text(text).unwrap();
        assert_eq!(name, "plan_mode_respond");
        assert_eq!(
            extract_tag_content(&result, "user_message"),
            Some("hello world".to_string())
        );
    }
}
