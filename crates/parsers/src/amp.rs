use crate::common::set_first;
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub struct AmpParser;

impl SessionParser for AmpParser {
    fn name(&self) -> &str {
        "amp"
    }

    fn can_parse(&self, path: &Path) -> bool {
        // Entry point: ~/.local/share/amp/threads/T-{uuid}.json
        path.extension().is_some_and(|ext| ext == "json")
            && path.to_str().is_some_and(|s| s.contains("amp/threads/"))
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_amp_thread(path)
    }
}

// ── Raw deserialization types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AmpThread {
    #[serde(default)]
    #[allow(dead_code)]
    v: u64,
    id: String,
    #[serde(default)]
    created: u64,
    messages: Vec<AmpMessage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AmpMessage {
    role: String,
    #[serde(default, rename = "messageId")]
    message_id: u64,
    content: Vec<AmpContentBlock>,
    #[serde(default)]
    state: Option<AmpState>,
    #[serde(default)]
    usage: Option<AmpUsage>,
    #[serde(default)]
    meta: Option<AmpMeta>,
    #[serde(default, rename = "agentMode")]
    agent_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum AmpContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
        #[serde(default)]
        provider: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        complete: Option<bool>,
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default, rename = "toolUseID")]
        tool_use_id: Option<String>,
        #[serde(default)]
        run: Option<serde_json::Value>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AmpState {
    #[serde(rename = "type")]
    state_type: Option<String>,
    #[serde(default, rename = "stopReason")]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AmpUsage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "inputTokens")]
    input_tokens: u64,
    #[serde(default, rename = "outputTokens")]
    output_tokens: u64,
    #[serde(default)]
    credits: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AmpMeta {
    #[serde(default, rename = "sentAt")]
    sent_at: Option<u64>,
}

// ── Parsing logic ───────────────────────────────────────────────────────────

fn parse_amp_thread(path: &Path) -> Result<Session> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Amp thread: {}", path.display()))?;
    let thread: AmpThread = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse Amp thread: {}", path.display()))?;

    let mut events: Vec<Event> = Vec::new();
    let mut event_counter = 0u64;
    let mut model: Option<String> = None;
    let mut first_user_text: Option<String> = None;

    // Track last tool_use name/id for pairing with tool_result
    let mut last_tool_name = "unknown".to_string();
    let mut last_tool_event_id = String::new();

    for msg in &thread.messages {
        // Extract model from assistant usage
        if let Some(ref usage) = msg.usage {
            set_first(&mut model, usage.model.clone());
        }

        // Build token attributes from usage data
        let mut token_attrs = HashMap::new();
        if let Some(ref usage) = msg.usage {
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

        // Approximate timestamp from message meta or thread creation
        let msg_ts = msg
            .meta
            .as_ref()
            .and_then(|m| m.sent_at)
            .map(millis_to_datetime)
            .unwrap_or_else(|| millis_to_datetime(thread.created + msg.message_id * 1000));

        // Track whether we've emitted the first AgentMessage for this turn
        // (token_attrs should only be attached once per message)
        let mut tokens_emitted = false;

        for block in &msg.content {
            match block {
                AmpContentBlock::Text { text } => {
                    if text.is_empty() {
                        continue;
                    }

                    let event_type = match msg.role.as_str() {
                        "user" => {
                            set_first(&mut first_user_text, Some(text.clone()));
                            EventType::UserMessage
                        }
                        "assistant" => EventType::AgentMessage,
                        _ => continue,
                    };

                    let attrs = if !tokens_emitted
                        && matches!(event_type, EventType::AgentMessage)
                        && !token_attrs.is_empty()
                    {
                        tokens_emitted = true;
                        token_attrs.clone()
                    } else {
                        HashMap::new()
                    };

                    event_counter += 1;
                    events.push(Event {
                        event_id: format!("amp-{}", event_counter),
                        timestamp: msg_ts,
                        event_type,
                        task_id: None,
                        content: Content::text(text),
                        duration_ms: None,
                        attributes: attrs,
                    });
                }
                AmpContentBlock::Thinking { thinking, .. } => {
                    if thinking.is_empty() {
                        continue;
                    }
                    event_counter += 1;
                    events.push(Event {
                        event_id: format!("amp-{}", event_counter),
                        timestamp: msg_ts,
                        event_type: EventType::Thinking,
                        task_id: None,
                        content: Content::text(thinking),
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }
                AmpContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    last_tool_name = name.clone();
                    let event_type = classify_amp_tool(name, input);
                    let content = amp_tool_content(name, input);
                    event_counter += 1;
                    let eid = id
                        .clone()
                        .unwrap_or_else(|| format!("amp-{}", event_counter));
                    last_tool_event_id = eid.clone();
                    events.push(Event {
                        event_id: eid,
                        timestamp: msg_ts,
                        event_type,
                        task_id: None,
                        content,
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }
                AmpContentBlock::ToolResult {
                    tool_use_id, run, ..
                } => {
                    let output = extract_amp_tool_result(run);
                    let is_error = run
                        .as_ref()
                        .and_then(|r| r.get("status").and_then(|s| s.as_str()))
                        .is_some_and(|s| s == "error");
                    let call_id = tool_use_id.clone().or_else(|| {
                        if last_tool_event_id.is_empty() {
                            None
                        } else {
                            Some(last_tool_event_id.clone())
                        }
                    });

                    event_counter += 1;
                    events.push(Event {
                        event_id: format!("amp-{}", event_counter),
                        timestamp: msg_ts,
                        event_type: EventType::ToolResult {
                            name: last_tool_name.clone(),
                            is_error,
                            call_id,
                        },
                        task_id: None,
                        content: Content::text(&output),
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }
                AmpContentBlock::Unknown => {}
            }
        }
    }

    let created_at = millis_to_datetime(thread.created);
    let updated_at = events.last().map(|e| e.timestamp).unwrap_or(created_at);

    // Derive model provider from model name
    let model_str = model.unwrap_or_else(|| "unknown".to_string());
    let provider = if model_str.contains("claude") {
        "anthropic".to_string()
    } else if model_str.contains("gpt") || model_str.contains("o1") || model_str.contains("o3") {
        "openai".to_string()
    } else if model_str.contains("gemini") {
        "google".to_string()
    } else {
        "unknown".to_string()
    };

    // Title: first user message truncated
    let title = first_user_text.map(|t| {
        if t.chars().count() > 80 {
            let truncated: String = t.chars().take(77).collect();
            format!("{}...", truncated)
        } else {
            t
        }
    });

    let agent = Agent {
        provider,
        model: model_str,
        tool: "amp".to_string(),
        tool_version: None,
    };

    let context = SessionContext {
        title,
        description: None,
        tags: vec!["amp".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes: HashMap::new(),
    };

    let mut session = Session::new(thread.id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

fn millis_to_datetime(ms: u64) -> chrono::DateTime<Utc> {
    let secs = (ms / 1000) as i64;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nsecs)
        .single()
        .unwrap_or_else(Utc::now)
}

fn classify_amp_tool(name: &str, input: &serde_json::Value) -> EventType {
    match name {
        "Bash" | "bash" | "Terminal" => {
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
        "Edit" | "edit" | "str_replace_editor" => {
            let path = input
                .get("path")
                .or_else(|| input.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "Write" | "write" | "create_file" => {
            let path = input
                .get("path")
                .or_else(|| input.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileCreate { path }
        }
        "Read" | "read" => {
            let path = input
                .get("path")
                .or_else(|| input.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        "Grep" | "grep" => {
            let query = input
                .get("pattern")
                .or_else(|| input.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::CodeSearch { query }
        }
        "Glob" | "glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            EventType::FileSearch { pattern }
        }
        "WebFetch" | "web_fetch" => {
            let url = input
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::WebFetch { url }
        }
        "WebSearch" | "web_search" => {
            let query = input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::WebSearch { query }
        }
        "todo_write" | "TodoWrite" => EventType::ToolCall {
            name: "todo_write".to_string(),
        },
        _ => EventType::ToolCall {
            name: name.to_string(),
        },
    }
}

fn amp_tool_content(name: &str, input: &serde_json::Value) -> Content {
    match name {
        "Bash" | "bash" | "Terminal" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            Content {
                blocks: vec![ContentBlock::Code {
                    code: cmd.to_string(),
                    language: Some("bash".to_string()),
                    start_line: None,
                }],
            }
        }
        _ => Content {
            blocks: vec![ContentBlock::Json {
                data: input.clone(),
            }],
        },
    }
}

fn extract_amp_tool_result(run: &Option<serde_json::Value>) -> String {
    let run = match run {
        Some(r) => r,
        None => return String::new(),
    };

    // Tool result in Amp can be in run.result (string or object)
    if let Some(result) = run.get("result") {
        if let Some(s) = result.as_str() {
            return s.to_string();
        }
        // For Read tool: result.content
        if let Some(content) = result.get("content").and_then(|c| c.as_str()) {
            return content.to_string();
        }
        return result.to_string();
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_bash() {
        let input = serde_json::json!({"command": "npm test"});
        let et = classify_amp_tool("Bash", &input);
        match et {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "npm test"),
            _ => panic!("Expected ShellCommand"),
        }
    }

    #[test]
    fn test_classify_create_file() {
        let input = serde_json::json!({"file_path": "index.html", "content": "<html>"});
        let et = classify_amp_tool("create_file", &input);
        match et {
            EventType::FileCreate { path } => assert_eq!(path, "index.html"),
            _ => panic!("Expected FileCreate"),
        }
    }

    #[test]
    fn test_millis_to_datetime() {
        let dt = millis_to_datetime(1768200620466);
        assert!(dt.year() >= 2025);
    }

    use chrono::Datelike;
}
