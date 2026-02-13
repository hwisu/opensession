use crate::common::set_first;
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
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

// ── Parsing logic ───────────────────────────────────────────────────────────
//
// Codex CLI JSONL format:
//   Line 1:  {id, timestamp, instructions, git?}     — session header (no `type` field)
//   Line 2+: {record_type: "state"}                   — state markers (skip)
//   Line 3+: {type: "message"|"reasoning"|"function_call"|..., ...}  — entries
//
// Model is NOT stored in the JSONL — it's in ~/.codex/config.toml globally.

fn parse_codex_jsonl(path: &Path) -> Result<Session> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open Codex JSONL: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut session_id: Option<String> = None;
    let mut event_counter = 0u64;
    let mut first_user_text: Option<String> = None;
    let mut last_function_name = "unknown".to_string();
    // call_id → (event_id, function_name) for correlating function_call_output
    let mut call_map: HashMap<String, (String, String)> = HashMap::new();
    let mut session_ts: Option<DateTime<Utc>> = None;
    let mut git_info: Option<serde_json::Value> = None;
    let mut cwd: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut originator: Option<String> = None;
    let mut is_desktop = false;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let obj = match v.as_object() {
            Some(o) => o,
            None => continue,
        };

        // State marker — skip
        if obj.contains_key("record_type") {
            continue;
        }

        // Codex Desktop "session_meta" header (has `type: "session_meta"` + `payload`)
        if obj.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
            is_desktop = true;
            if let Some(payload) = obj.get("payload") {
                set_first(
                    &mut session_id,
                    payload.get("id").and_then(|v| v.as_str()).map(String::from),
                );
                if let Some(ts_str) = payload.get("timestamp").and_then(|v| v.as_str()) {
                    set_first(&mut session_ts, parse_timestamp(ts_str).ok());
                }
                if let Some(git) = payload.get("git") {
                    set_first(&mut git_info, Some(git.clone()));
                }
                set_first(
                    &mut cwd,
                    payload
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                );
                set_first(
                    &mut tool_version,
                    payload
                        .get("cli_version")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                );
                set_first(
                    &mut originator,
                    payload
                        .get("originator")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                );
            }
            continue;
        }

        // Session header — no `type` field, has `id` + `timestamp` (legacy CLI format)
        if !obj.contains_key("type") {
            set_first(
                &mut session_id,
                obj.get("id").and_then(|v| v.as_str()).map(String::from),
            );
            if let Some(ts_str) = obj.get("timestamp").and_then(|v| v.as_str()) {
                set_first(&mut session_ts, parse_timestamp(ts_str).ok());
            }
            if let Some(git) = obj.get("git") {
                git_info = Some(git.clone());
            }
            continue;
        }

        let top_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

        // Per-entry timestamp (Desktop format includes timestamp on each line)
        let entry_ts = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| parse_timestamp(s).ok())
            .or(session_ts)
            .unwrap_or_else(Utc::now);

        // Codex Desktop: `response_item` wraps the payload which has the same
        // structure as legacy flat entries (message, reasoning, function_call, etc.)
        if top_type == "response_item" {
            if let Some(payload) = obj.get("payload") {
                // In Desktop format, response_item/message/role=user includes
                // system-injected content (AGENTS.md, env context). The real user
                // message comes from event_msg/user_message, so skip first_user_text
                // extraction here for Desktop sessions.
                let mut discard_user_text: Option<String> = None;
                let user_text_target = if is_desktop {
                    &mut discard_user_text
                } else {
                    &mut first_user_text
                };
                process_item(
                    payload,
                    entry_ts,
                    &mut events,
                    &mut event_counter,
                    user_text_target,
                    &mut last_function_name,
                    &mut call_map,
                );
            }
            continue;
        }

        // Codex Desktop: `event_msg` contains UI-level events
        if top_type == "event_msg" {
            if let Some(payload) = obj.get("payload") {
                let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if payload_type == "user_message" {
                    if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
                        let text = msg.trim().to_string();
                        if !text.is_empty() {
                            // For Desktop, event_msg/user_message is the authoritative
                            // source for user text. Overwrite even if already set.
                            if first_user_text.is_none() {
                                first_user_text = Some(text);
                            }
                        }
                    }
                }
            }
            continue;
        }

        // Skip other Desktop-only wrapper types
        if top_type == "turn_context" {
            continue;
        }

        // Legacy flat entry with type field (message, reasoning, function_call, etc.)
        process_item(
            &v,
            entry_ts,
            &mut events,
            &mut event_counter,
            &mut first_user_text,
            &mut last_function_name,
            &mut call_map,
        );
    }

    // ── Build Session ───────────────────────────────────────────────────────

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let agent = Agent {
        provider: "openai".to_string(),
        model: "unknown".to_string(),
        tool: "codex".to_string(),
        tool_version,
    };

    let (created_at, updated_at) =
        if let (Some(first), Some(last)) = (events.first(), events.last()) {
            (first.timestamp, last.timestamp)
        } else {
            let now = session_ts.unwrap_or_else(Utc::now);
            (now, now)
        };

    let mut attributes = HashMap::new();
    if let Some(git) = git_info {
        attributes.insert("git".to_string(), git);
    }
    if let Some(ref dir) = cwd {
        attributes.insert("cwd".to_string(), serde_json::Value::String(dir.clone()));
    }
    if let Some(ref orig) = originator {
        attributes.insert(
            "originator".to_string(),
            serde_json::Value::String(orig.clone()),
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
        tags: vec!["codex".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

/// Process a flat entry with `type` at the top level.
fn process_item(
    item: &serde_json::Value,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    counter: &mut u64,
    first_user_text: &mut Option<String>,
    last_function_name: &mut String,
    call_map: &mut HashMap<String, (String, String)>,
) {
    let item_type = match item.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return,
    };

    match item_type {
        "message" => {
            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content_blocks = item
                .get("content")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

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
                return;
            }

            let event_type = match role {
                "user" => EventType::UserMessage,
                "assistant" => EventType::AgentMessage,
                "developer" | "system" => return,
                _ => return,
            };

            if role == "user" {
                set_first(first_user_text, Some(text.clone()));
            }

            *counter += 1;
            events.push(Event {
                event_id: format!("codex-{}", counter),
                timestamp: ts,
                event_type,
                task_id: None,
                content: Content::text(&text),
                duration_ms: None,
                attributes: HashMap::new(),
            });
        }
        "reasoning" => {
            let summaries = item
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
                *counter += 1;
                events.push(Event {
                    event_id: format!("codex-{}", counter),
                    timestamp: ts,
                    event_type: EventType::Thinking,
                    task_id: None,
                    content: Content::text(&text),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
        }
        "function_call" | "custom_tool_call" => {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            // function_call: arguments is a JSON string
            // custom_tool_call: input is a raw string (patch content, etc.)
            let args_str = item
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let args: serde_json::Value =
                serde_json::from_str(args_str).unwrap_or(serde_json::Value::Null);

            let event_type = classify_codex_function(&name, &args);
            let content = if item_type == "custom_tool_call" {
                // Custom tools store input as raw text (e.g. patch content)
                let input = item.get("input").and_then(|v| v.as_str()).unwrap_or("");
                Content::text(input)
            } else {
                codex_function_content(&name, &args)
            };

            *counter += 1;
            let event_id = format!("codex-{}", counter);

            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                call_map.insert(call_id.to_string(), (event_id.clone(), name.clone()));
            }
            *last_function_name = name;

            events.push(Event {
                event_id,
                timestamp: ts,
                event_type,
                task_id: None,
                content,
                duration_ms: None,
                attributes: HashMap::new(),
            });
        }
        "function_call_output" | "custom_tool_call_output" => {
            let raw_output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");

            let (output_text, is_error, duration_ms) = parse_function_output(raw_output);

            // Correlate with function_call via call_id
            let (call_id_ref, call_name) =
                if let Some(cid) = item.get("call_id").and_then(|v| v.as_str()) {
                    if let Some((eid, name)) = call_map.get(cid) {
                        (Some(eid.clone()), name.clone())
                    } else {
                        (None, last_function_name.clone())
                    }
                } else {
                    let prev_id = if *counter > 0 {
                        Some(format!("codex-{}", counter))
                    } else {
                        None
                    };
                    (prev_id, last_function_name.clone())
                };

            *counter += 1;
            events.push(Event {
                event_id: format!("codex-{}", counter),
                timestamp: ts,
                event_type: EventType::ToolResult {
                    name: call_name,
                    is_error,
                    call_id: call_id_ref,
                },
                task_id: None,
                content: Content::text(&output_text),
                duration_ms,
                attributes: HashMap::new(),
            });
        }
        "web_search_call" => {
            let url = item
                .get("action")
                .and_then(|a| a.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !url.is_empty() {
                *counter += 1;
                events.push(Event {
                    event_id: format!("codex-{}", counter),
                    timestamp: ts,
                    event_type: EventType::ToolCall {
                        name: "web_search".to_string(),
                    },
                    task_id: None,
                    content: Content::text(url),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
        }
        _ => {}
    }
}

/// Parse the output string from function_call_output.
/// Format: `{"output":"command output\n","metadata":{"exit_code":0,"duration_seconds":0.5}}`
/// Returns (text, is_error, duration_ms).
fn parse_function_output(raw: &str) -> (String, bool, Option<u64>) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        let output = v
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or(raw)
            .to_string();
        let exit_code = v
            .get("metadata")
            .and_then(|m| m.get("exit_code"))
            .and_then(|c| c.as_i64());
        let duration = v
            .get("metadata")
            .and_then(|m| m.get("duration_seconds"))
            .and_then(|d| d.as_f64())
            .map(|s| (s * 1000.0) as u64);
        let is_error = exit_code.is_some_and(|c| c != 0);
        (output, is_error, duration)
    } else {
        (raw.to_string(), false, None)
    }
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

/// Extract a shell command string from function arguments.
/// Handles: `{cmd: "..."}`, `{command: ["bash", "-lc", "cmd"]}`, `{command: "cmd"}`.
fn extract_shell_command(args: &serde_json::Value) -> String {
    if let Some(cmd) = args.get("cmd").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    if let Some(arr) = args.get("command").and_then(|v| v.as_array()) {
        let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        // Skip shell prefix (e.g. "bash -lc") and take the actual command
        if parts.len() >= 3 {
            return parts[2..].join(" ");
        }
        return parts.join(" ");
    }
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    String::new()
}

fn classify_codex_function(name: &str, args: &serde_json::Value) -> EventType {
    match name {
        "exec_command" | "shell" => {
            let cmd = extract_shell_command(args);
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
            let cmd = extract_shell_command(args);
            Content {
                blocks: vec![ContentBlock::Code {
                    code: cmd,
                    language: Some("bash".to_string()),
                    start_line: None,
                }],
            }
        }
        _ => Content {
            blocks: vec![ContentBlock::Json { data: args.clone() }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_header() {
        let line = r#"{"id":"c3c4b301-27c8-4c70-b6e4-46b99fdf0236","timestamp":"2025-08-18T01:16:13.522Z","instructions":null,"git":{"commit_hash":"abc123","branch":"main"}}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("type"));
        assert!(obj.contains_key("id"));
        assert_eq!(
            obj["id"].as_str().unwrap(),
            "c3c4b301-27c8-4c70-b6e4-46b99fdf0236"
        );
        assert!(obj["git"]["branch"].as_str().unwrap() == "main");
    }

    #[test]
    fn test_state_marker_skipped() {
        let line = r#"{"record_type":"state"}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("record_type"));
        assert!(!obj.contains_key("type"));
    }

    #[test]
    fn test_user_message() {
        let line = r#"{"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"hello codex"}]}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        process_item(
            &v,
            Utc::now(),
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::UserMessage));
        assert_eq!(first_text.as_deref(), Some("hello codex"));
    }

    #[test]
    fn test_assistant_message() {
        let line = r#"{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Here is the analysis..."}]}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        process_item(
            &v,
            Utc::now(),
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::AgentMessage));
    }

    #[test]
    fn test_shell_command_array() {
        let line = r#"{"type":"function_call","id":"fc_123","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cat README.md\"]}","call_id":"call_xyz"}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        process_item(
            &v,
            Utc::now(),
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        assert_eq!(events.len(), 1);
        match &events[0].event_type {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "cat README.md"),
            other => panic!("Expected ShellCommand, got {:?}", other),
        }
        assert!(call_map.contains_key("call_xyz"));
    }

    #[test]
    fn test_shell_command_single_element() {
        let args = serde_json::json!({"command": ["pwd"]});
        assert_eq!(extract_shell_command(&args), "pwd");
    }

    #[test]
    fn test_extract_shell_command_variants() {
        // Array with shell prefix
        let args = serde_json::json!({"command": ["bash", "-lc", "cargo test"], "workdir": "/tmp"});
        assert_eq!(extract_shell_command(&args), "cargo test");

        // Simple cmd field
        let args = serde_json::json!({"cmd": "cargo test"});
        assert_eq!(extract_shell_command(&args), "cargo test");

        // String command field
        let args = serde_json::json!({"command": "ls -la"});
        assert_eq!(extract_shell_command(&args), "ls -la");
    }

    #[test]
    fn test_parse_function_output_json() {
        let raw = r#"{"output":"hello world\n","metadata":{"exit_code":0,"duration_seconds":0.5}}"#;
        let (text, is_error, duration) = parse_function_output(raw);
        assert_eq!(text, "hello world\n");
        assert!(!is_error);
        assert_eq!(duration, Some(500));
    }

    #[test]
    fn test_parse_function_output_error() {
        let raw = r#"{"output":"command not found","metadata":{"exit_code":127,"duration_seconds":0.01}}"#;
        let (_, is_error, _) = parse_function_output(raw);
        assert!(is_error);
    }

    #[test]
    fn test_parse_function_output_plain() {
        let (text, is_error, duration) = parse_function_output("Plan updated");
        assert_eq!(text, "Plan updated");
        assert!(!is_error);
        assert!(duration.is_none());
    }

    #[test]
    fn test_call_id_correlation() {
        let call_line = r#"{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"echo hi\"]}","call_id":"call_abc"}"#;
        let output_line = r#"{"type":"function_call_output","call_id":"call_abc","output":"{\"output\":\"hi\\n\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.01}}"}"#;

        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        let ts = Utc::now();

        let v1: serde_json::Value = serde_json::from_str(call_line).unwrap();
        process_item(
            &v1,
            ts,
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );

        let v2: serde_json::Value = serde_json::from_str(output_line).unwrap();
        process_item(
            &v2,
            ts,
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );

        assert_eq!(events.len(), 2);
        match &events[1].event_type {
            EventType::ToolResult {
                name,
                is_error,
                call_id,
            } => {
                assert_eq!(name, "shell");
                assert!(!is_error);
                assert_eq!(call_id.as_deref(), Some("codex-1"));
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
        assert_eq!(events[1].duration_ms, Some(10));
    }

    #[test]
    fn test_reasoning_with_summary() {
        let line = r#"{"type":"reasoning","id":"rs_123","summary":[{"type":"summary_text","text":"Analyzing the code"}],"encrypted_content":"gAAAAA..."}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        process_item(
            &v,
            Utc::now(),
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::Thinking));
    }

    #[test]
    fn test_reasoning_empty_summary_skipped() {
        let line =
            r#"{"type":"reasoning","id":"rs_456","summary":[],"encrypted_content":"gAAAAA..."}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        process_item(
            &v,
            Utc::now(),
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_classify_update_plan() {
        let args = serde_json::json!({"plan": [{"step": "analyze", "status": "in_progress"}]});
        let et = classify_codex_function("update_plan", &args);
        assert!(matches!(et, EventType::ToolCall { name } if name == "update_plan"));
    }

    #[test]
    fn test_desktop_format_response_item() {
        // Desktop wraps entries in response_item with payload
        let lines = vec![
            r#"{"timestamp":"2026-02-03T04:11:00.097Z","type":"session_meta","payload":{"id":"desktop-test","timestamp":"2026-02-03T04:11:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-03T04:11:00.097Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"system instructions"}]}}"#,
            r#"{"timestamp":"2026-02-03T04:11:00.097Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"AGENTS.md instructions"}]}}"#,
            r#"{"timestamp":"2026-02-03T04:11:00.119Z","type":"event_msg","payload":{"type":"user_message","message":"fix the bug"}}"#,
            r#"{"timestamp":"2026-02-03T04:11:03.355Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Analyzing"}]}}"#,
            r#"{"timestamp":"2026-02-03T04:11:03.624Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"call_1"}}"#,
            r#"{"timestamp":"2026-02-03T04:11:04.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_1","output":"{\"output\":\"file.txt\\n\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.1}}"}}"#,
            r#"{"timestamp":"2026-02-03T04:11:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let _parser = CodexParser;
        // can_parse won't match (no .codex/sessions in path), so call parse directly
        let session = parse_codex_jsonl(&path).unwrap();

        assert_eq!(session.session_id, "desktop-test");
        assert_eq!(session.agent.tool, "codex");
        // Title should come from event_msg/user_message, not AGENTS.md
        assert_eq!(session.context.title.as_deref(), Some("fix the bug"));
        // developer messages are skipped, user messages from response_item are kept
        // Events: user(AGENTS.md) + reasoning + shell_command + tool_result + assistant
        assert!(session.events.len() >= 4);
        // Check originator attribute
        assert_eq!(
            session
                .context
                .attributes
                .get("originator")
                .and_then(|v| v.as_str()),
            Some("Codex Desktop")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
