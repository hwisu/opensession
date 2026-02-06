use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

pub struct CodexParser;

impl SessionParser for CodexParser {
    fn name(&self) -> &str {
        "codex"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "jsonl")
            && path
                .to_str()
                .is_some_and(|s| s.contains(".codex/sessions") || s.contains("codex/sessions"))
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_codex_jsonl(path)
    }
}

// ── Raw JSONL deserialization types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RawLine {
    timestamp: String,
    #[serde(rename = "type")]
    entry_type: String,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SessionMeta {
    id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    originator: Option<String>,
    #[serde(default)]
    cli_version: Option<String>,
    #[serde(default)]
    model_provider: Option<String>,
}

// ── Parsing logic ───────────────────────────────────────────────────────────

fn parse_codex_jsonl(path: &Path) -> Result<Session> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open Codex JSONL: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut cli_version: Option<String> = None;
    let mut model_provider: Option<String> = None;
    let mut event_counter = 0u64;
    let mut last_function_name = "unknown".to_string();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let raw: RawLine = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let ts = parse_timestamp(&raw.timestamp).unwrap_or_else(|_| Utc::now());

        match raw.entry_type.as_str() {
            "session_meta" => {
                if let Ok(meta) = serde_json::from_value::<SessionMeta>(raw.payload) {
                    session_id = Some(meta.id);
                    cwd = meta.cwd;
                    cli_version = meta.cli_version;
                    model_provider = meta.model_provider;
                }
            }
            "event_msg" => {
                let payload_type = raw.payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match payload_type {
                    "user_message" => {
                        let message = raw
                            .payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !message.is_empty() {
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: ts,
                                event_type: EventType::UserMessage,
                                task_id: None,
                                content: Content::text(&message),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    "agent_message" => {
                        let message = raw
                            .payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !message.is_empty() {
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: ts,
                                event_type: EventType::AgentMessage,
                                task_id: None,
                                content: Content::text(&message),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    "agent_reasoning" => {
                        let text = raw
                            .payload
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !text.is_empty() {
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: ts,
                                event_type: EventType::Thinking,
                                task_id: None,
                                content: Content::text(&text),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "response_item" => {
                let payload_type = raw.payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match payload_type {
                    "message" => {
                        let role = raw
                            .payload
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let content_blocks = raw
                            .payload
                            .get("content")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        // Extract text from content blocks
                        let text: String = content_blocks
                            .iter()
                            .filter_map(|b| {
                                let btype = b.get("type").and_then(|v| v.as_str())?;
                                match btype {
                                    "output_text" | "input_text" => {
                                        b.get("text").and_then(|v| v.as_str()).map(String::from)
                                    }
                                    _ => None,
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        if text.is_empty() {
                            continue;
                        }

                        let event_type = match role {
                            "user" => EventType::UserMessage,
                            "assistant" => EventType::AgentMessage,
                            "developer" | "system" => EventType::SystemMessage,
                            _ => continue,
                        };

                        // Skip system/developer messages (they are prompts, not conversation)
                        if matches!(role, "developer" | "system") {
                            continue;
                        }

                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type,
                            task_id: None,
                            content: Content::text(&text),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    "reasoning" => {
                        let summaries = raw
                            .payload
                            .get("summary")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let text: String = summaries
                            .iter()
                            .filter_map(|s| {
                                let stype = s.get("type").and_then(|v| v.as_str())?;
                                if stype == "summary_text" {
                                    s.get("text").and_then(|v| v.as_str()).map(String::from)
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        if !text.is_empty() {
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: ts,
                                event_type: EventType::Thinking,
                                task_id: None,
                                content: Content::text(&text),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    "function_call" => {
                        let name = raw
                            .payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let args_str = raw
                            .payload
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let args: serde_json::Value =
                            serde_json::from_str(args_str).unwrap_or(serde_json::Value::Null);

                        let event_type = classify_codex_function(&name, &args);
                        let content = codex_function_content(&name, &args);
                        last_function_name = name;

                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type,
                            task_id: None,
                            content,
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    "function_call_output" => {
                        let output = raw
                            .payload
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Link to preceding function_call event
                        let call_id = if event_counter > 0 {
                            Some(format!("codex-{}", event_counter))
                        } else {
                            None
                        };
                        let call_name = last_function_name.clone();
                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: call_name,
                                is_error: output.contains("failed"),
                                call_id,
                            },
                            task_id: None,
                            content: Content::text(&output),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    "custom_tool_call" => {
                        let name = raw
                            .payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("custom_tool")
                            .to_string();
                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolCall { name },
                            task_id: None,
                            content: Content::empty(),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    "custom_tool_call_output" => {
                        let call_id = if event_counter > 0 {
                            Some(format!("codex-{}", event_counter))
                        } else {
                            None
                        };
                        event_counter += 1;
                        let output = raw
                            .payload
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: "custom_tool".to_string(),
                                is_error: false,
                                call_id,
                            },
                            task_id: None,
                            content: Content::text(&output),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    "web_search_call" => {
                        let query = raw
                            .payload
                            .get("query")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::WebSearch { query },
                            task_id: None,
                            content: Content::empty(),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // Detect model from events or filename
    let provider = model_provider.unwrap_or_else(|| "openai".to_string());

    let agent = Agent {
        provider,
        model: "gpt-4o".to_string(),
        tool: "codex".to_string(),
        tool_version: cli_version,
    };

    let (created_at, updated_at) = if let (Some(first), Some(last)) =
        (events.first(), events.last())
    {
        (first.timestamp, last.timestamp)
    } else {
        let now = Utc::now();
        (now, now)
    };

    let mut attributes = HashMap::new();
    if let Some(ref dir) = cwd {
        attributes.insert(
            "cwd".to_string(),
            serde_json::Value::String(dir.clone()),
        );
    }

    let context = SessionContext {
        title: None,
        description: None,
        tags: vec!["codex".to_string()],
        created_at,
        updated_at,
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
        })
        .with_context(|| format!("Failed to parse timestamp: {}", ts))
}

fn classify_codex_function(name: &str, args: &serde_json::Value) -> EventType {
    match name {
        "exec_command" | "shell" => {
            let cmd = args
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::ShellCommand {
                command: cmd,
                exit_code: None,
            }
        }
        "write_stdin" => EventType::ToolCall {
            name: "write_stdin".to_string(),
        },
        "apply_diff" | "apply_patch" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "create_file" | "write_file" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileCreate { path }
        }
        "read_file" => {
            let path = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        _ => EventType::ToolCall {
            name: name.to_string(),
        },
    }
}

fn codex_function_content(name: &str, args: &serde_json::Value) -> Content {
    match name {
        "exec_command" | "shell" => {
            let cmd = args
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
                data: args.clone(),
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_meta() {
        let line = r#"{"timestamp":"2026-02-03T04:11:00.097Z","type":"session_meta","payload":{"id":"abc-123","cwd":"/tmp","cli_version":"0.94.0","model_provider":"openai"}}"#;
        let raw: RawLine = serde_json::from_str(line).unwrap();
        assert_eq!(raw.entry_type, "session_meta");
        let meta: SessionMeta = serde_json::from_value(raw.payload).unwrap();
        assert_eq!(meta.id, "abc-123");
        assert_eq!(meta.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn test_parse_user_message() {
        let line = r#"{"timestamp":"2026-02-03T04:11:00.097Z","type":"event_msg","payload":{"type":"user_message","message":"hello codex"}}"#;
        let raw: RawLine = serde_json::from_str(line).unwrap();
        assert_eq!(raw.entry_type, "event_msg");
        let msg = raw.payload.get("message").unwrap().as_str().unwrap();
        assert_eq!(msg, "hello codex");
    }

    #[test]
    fn test_classify_exec_command() {
        let args = serde_json::json!({"cmd": "cargo test"});
        let et = classify_codex_function("exec_command", &args);
        match et {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "cargo test"),
            _ => panic!("Expected ShellCommand"),
        }
    }
}
