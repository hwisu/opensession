use crate::common::set_first;
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub struct GeminiParser;

impl SessionParser for GeminiParser {
    fn name(&self) -> &str {
        "gemini"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext == "json" || ext == "jsonl")
            && path
                .to_str()
                .is_some_and(|s| s.contains(".gemini/tmp/") && s.contains("/chats/session-"))
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            parse_jsonl(path)
        } else {
            parse_json(path)
        }
    }
}

// ── Deserialization types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSession {
    session_id: String,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    messages: Vec<GeminiMessage>,
}

#[derive(Debug, Deserialize)]
struct GeminiMessage {
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    thoughts: Option<Vec<GeminiThought>>,
    #[allow(dead_code)]
    #[serde(default)]
    tokens: Option<GeminiTokens>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiThought {
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GeminiTokens {
    #[serde(default)]
    input: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
    #[serde(default)]
    cached: Option<u64>,
    #[serde(default)]
    thoughts: Option<u64>,
    #[serde(default)]
    tool: Option<u64>,
    #[serde(default)]
    total: Option<u64>,
}

// ── Parsing logic ───────────────────────────────────────────────────────────
//
// Gemini CLI session format:
//   Location: ~/.gemini/tmp/<project_hash>/chats/session-*.json
//   Single JSON file per session with a messages array.
//   Message types: "user", "gemini", "error", "info"
//   Model info on gemini messages: message.model
//   Thinking in message.thoughts array.

fn parse_json(path: &Path) -> Result<Session> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Gemini session: {}", path.display()))?;
    let session: GeminiSession = serde_json::from_str(&text)
        .with_context(|| format!("Failed to parse Gemini session: {}", path.display()))?;

    let mut events: Vec<Event> = Vec::new();
    let mut event_counter = 0u64;
    let mut first_user_text: Option<String> = None;
    let mut model_name: Option<String> = None;

    for msg in &session.messages {
        let ts = msg
            .timestamp
            .as_deref()
            .and_then(|s| parse_timestamp(s).ok())
            .unwrap_or_else(Utc::now);

        let content_text = msg.content.as_deref().unwrap_or("");

        match msg.msg_type.as_str() {
            "user" => {
                if content_text.is_empty() {
                    continue;
                }
                set_first(&mut first_user_text, Some(content_text.to_string()));
                event_counter += 1;
                events.push(Event {
                    event_id: format!("gemini-{}", event_counter),
                    timestamp: ts,
                    event_type: EventType::UserMessage,
                    task_id: None,
                    content: Content::text(content_text),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
            "gemini" => {
                // Extract model
                set_first(&mut model_name, msg.model.clone());

                // Build token attributes from usage data
                let mut token_attrs = HashMap::new();
                if let Some(ref tokens) = msg.tokens {
                    if let Some(input) = tokens.input {
                        token_attrs.insert(
                            "input_tokens".to_string(),
                            serde_json::Value::Number(input.into()),
                        );
                    }
                    if let Some(output) = tokens.output {
                        token_attrs.insert(
                            "output_tokens".to_string(),
                            serde_json::Value::Number(output.into()),
                        );
                    }
                }

                // Emit thinking events from thoughts
                if let Some(thoughts) = &msg.thoughts {
                    for thought in thoughts {
                        let text = match (&thought.subject, &thought.description) {
                            (Some(s), Some(d)) => format!("**{}**\n{}", s, d),
                            (Some(s), None) => s.clone(),
                            (None, Some(d)) => d.clone(),
                            (None, None) => continue,
                        };
                        event_counter += 1;
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::Thinking,
                            task_id: None,
                            content: Content::text(&text),
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                }

                // Emit agent message
                if !content_text.is_empty() {
                    event_counter += 1;
                    events.push(Event {
                        event_id: format!("gemini-{}", event_counter),
                        timestamp: ts,
                        event_type: EventType::AgentMessage,
                        task_id: None,
                        content: Content::text(content_text),
                        duration_ms: None,
                        attributes: token_attrs,
                    });
                }
            }
            "error" => {
                if !content_text.is_empty() {
                    event_counter += 1;
                    let mut attrs = HashMap::new();
                    attrs.insert("error".to_string(), serde_json::Value::Bool(true));
                    events.push(Event {
                        event_id: format!("gemini-{}", event_counter),
                        timestamp: ts,
                        event_type: EventType::AgentMessage,
                        task_id: None,
                        content: Content::text(content_text),
                        duration_ms: None,
                        attributes: attrs,
                    });
                }
            }
            // Skip "info" messages (auth, system notifications)
            _ => {}
        }
    }

    // Build session
    let model = model_name.unwrap_or_else(|| "unknown".to_string());
    let agent = Agent {
        provider: "google".to_string(),
        model,
        tool: "gemini".to_string(),
        tool_version: None,
    };

    let created_at = session
        .start_time
        .as_deref()
        .and_then(|s| parse_timestamp(s).ok())
        .or_else(|| events.first().map(|e| e.timestamp))
        .unwrap_or_else(Utc::now);
    let updated_at = session
        .last_updated
        .as_deref()
        .and_then(|s| parse_timestamp(s).ok())
        .or_else(|| events.last().map(|e| e.timestamp))
        .unwrap_or(created_at);

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
        tags: vec!["gemini".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes: HashMap::new(),
    };

    let mut session_out = Session::new(session.session_id, agent);
    session_out.context = context;
    session_out.events = events;
    session_out.recompute_stats();

    Ok(session_out)
}

// ── JSONL format types ──────────────────────────────────────────────────────
//
// Gemini CLI JSONL session format (newer):
//   Location: ~/.gemini/tmp/<project_hash>/chats/session-*.jsonl
//   One JSON record per line. Record types: session_metadata, user, gemini, message_update.
//   Content is an array of typed blocks (text, functionCall, thinking, functionResponse).

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum GeminiRecord {
    #[serde(rename = "session_metadata")]
    SessionMetadata {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(default, rename = "startTime")]
        start_time: Option<String>,
    },
    #[serde(rename = "user")]
    User {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        content: Vec<GeminiContentBlock>,
    },
    #[serde(rename = "gemini")]
    Gemini {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(default)]
        content: Vec<GeminiContentBlock>,
        #[serde(default)]
        model: Option<String>,
    },
    #[serde(rename = "message_update")]
    MessageUpdate {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        tokens: Option<GeminiTokens>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum GeminiContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "functionCall")]
    FunctionCall {
        name: String,
        #[serde(default)]
        args: Option<serde_json::Value>,
    },
    #[serde(rename = "functionResponse")]
    FunctionResponse {
        name: String,
        #[serde(default)]
        response: Option<serde_json::Value>,
    },
    #[serde(rename = "thinking")]
    Thinking { text: String },
    #[serde(other)]
    Unknown,
}

fn parse_jsonl(path: &Path) -> Result<Session> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Gemini JSONL session: {}", path.display()))?;

    let mut session_id = None;
    let mut start_time = None;
    let mut events: Vec<Event> = Vec::new();
    let mut event_counter = 0u64;
    let mut first_user_text: Option<String> = None;
    let mut model_name: Option<String> = None;
    let mut token_map: HashMap<String, GeminiTokens> = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let record: GeminiRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Skipping unparseable Gemini JSONL line: {}", e);
                continue;
            }
        };

        match record {
            GeminiRecord::SessionMetadata {
                session_id: sid,
                start_time: st,
            } => {
                session_id = Some(sid);
                start_time = st;
            }
            GeminiRecord::User {
                id,
                timestamp,
                content,
            } => {
                let ts = timestamp
                    .as_deref()
                    .and_then(|s| parse_timestamp(s).ok())
                    .unwrap_or_else(Utc::now);

                // Collect text content from blocks
                let texts: Vec<&str> = content
                    .iter()
                    .filter_map(|b| match b {
                        GeminiContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let text_content = texts.join("\n");

                if !text_content.is_empty() {
                    set_first(&mut first_user_text, Some(text_content.clone()));
                    event_counter += 1;
                    events.push(Event {
                        event_id: id
                            .clone()
                            .unwrap_or_else(|| format!("gemini-{}", event_counter)),
                        timestamp: ts,
                        event_type: EventType::UserMessage,
                        task_id: None,
                        content: Content::text(&text_content),
                        duration_ms: None,
                        attributes: HashMap::new(),
                    });
                }

                // Handle functionResponse blocks in user messages (tool results)
                for block in &content {
                    if let GeminiContentBlock::FunctionResponse { name, response } = block {
                        event_counter += 1;
                        let result_content = match response {
                            Some(v) => Content {
                                blocks: vec![ContentBlock::Json { data: v.clone() }],
                            },
                            None => Content::text(""),
                        };
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: name.clone(),
                                is_error: false,
                                call_id: None,
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                }
            }
            GeminiRecord::Gemini {
                id,
                timestamp,
                content,
                model,
            } => {
                let ts = timestamp
                    .as_deref()
                    .and_then(|s| parse_timestamp(s).ok())
                    .unwrap_or_else(Utc::now);

                set_first(&mut model_name, model);

                let event_base_id =
                    id.unwrap_or_else(|| format!("gemini-auto-{}", event_counter + 1));

                for block in &content {
                    match block {
                        GeminiContentBlock::Thinking { text } => {
                            if !text.is_empty() {
                                event_counter += 1;
                                events.push(Event {
                                    event_id: format!("gemini-{}", event_counter),
                                    timestamp: ts,
                                    event_type: EventType::Thinking,
                                    task_id: None,
                                    content: Content::text(text),
                                    duration_ms: None,
                                    attributes: HashMap::new(),
                                });
                            }
                        }
                        GeminiContentBlock::Text { text } => {
                            if !text.is_empty() {
                                event_counter += 1;
                                events.push(Event {
                                    event_id: event_base_id.clone(),
                                    timestamp: ts,
                                    event_type: EventType::AgentMessage,
                                    task_id: None,
                                    content: Content::text(text),
                                    duration_ms: None,
                                    attributes: HashMap::new(),
                                });
                            }
                        }
                        GeminiContentBlock::FunctionCall { name, args } => {
                            event_counter += 1;
                            let call_content = match args {
                                Some(v) => Content {
                                    blocks: vec![ContentBlock::Json { data: v.clone() }],
                                },
                                None => Content::text(""),
                            };
                            events.push(Event {
                                event_id: format!("gemini-{}", event_counter),
                                timestamp: ts,
                                event_type: EventType::ToolCall { name: name.clone() },
                                task_id: None,
                                content: call_content,
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            GeminiRecord::MessageUpdate { id, tokens } => {
                if let (Some(id), Some(tokens)) = (id, tokens) {
                    token_map.insert(id, tokens);
                }
            }
        }
    }

    // Merge token info from message_update records into matching events
    for event in &mut events {
        if let Some(tokens) = token_map.get(&event.event_id) {
            if let Some(input) = tokens.input {
                event.attributes.insert(
                    "input_tokens".to_string(),
                    serde_json::Value::Number(input.into()),
                );
            }
            if let Some(output) = tokens.output {
                event.attributes.insert(
                    "output_tokens".to_string(),
                    serde_json::Value::Number(output.into()),
                );
            }
        }
    }

    // Build session
    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let model = model_name.unwrap_or_else(|| "unknown".to_string());
    let agent = Agent {
        provider: "google".to_string(),
        model,
        tool: "gemini".to_string(),
        tool_version: None,
    };

    let created_at = start_time
        .as_deref()
        .and_then(|s| parse_timestamp(s).ok())
        .or_else(|| events.first().map(|e| e.timestamp))
        .unwrap_or_else(Utc::now);
    let updated_at = events.last().map(|e| e.timestamp).unwrap_or(created_at);

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
        tags: vec!["gemini".to_string()],
        created_at,
        updated_at,
        related_session_ids: Vec::new(),
        attributes: HashMap::new(),
    };

    let mut session_out = Session::new(session_id, agent);
    session_out.context = context;
    session_out.events = events;
    session_out.recompute_stats();

    Ok(session_out)
}

fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .with_context(|| format!("Failed to parse timestamp: {}", ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_parse() {
        let parser = GeminiParser;
        assert!(parser.can_parse(Path::new(
            "/Users/test/.gemini/tmp/abc123/chats/session-2026-02-09T15-11-8205f040.json"
        )));
        assert!(parser.can_parse(Path::new(
            "/Users/test/.gemini/tmp/abc123/chats/session-2026-02-09T15-11-8205f040.jsonl"
        )));
        assert!(!parser.can_parse(Path::new("/tmp/random.json")));
        assert!(!parser.can_parse(Path::new("/tmp/random.jsonl")));
        assert!(!parser.can_parse(Path::new("/Users/test/.gemini/settings.json")));
    }

    #[test]
    fn test_parse_session() {
        let json = r#"{
            "sessionId": "test-123",
            "projectHash": "abc",
            "startTime": "2026-02-09T15:11:31.319Z",
            "lastUpdated": "2026-02-09T15:14:17.522Z",
            "messages": [
                {
                    "id": "m1",
                    "timestamp": "2026-02-09T15:11:31.319Z",
                    "type": "user",
                    "content": "hello gemini"
                },
                {
                    "id": "m2",
                    "timestamp": "2026-02-09T15:12:00.000Z",
                    "type": "gemini",
                    "content": "Hello! How can I help?",
                    "thoughts": [
                        {"subject": "Greeting", "description": "User says hello"}
                    ],
                    "tokens": {"input": 100, "output": 50, "total": 150},
                    "model": "gemini-2.5-pro"
                }
            ]
        }"#;

        let session: GeminiSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.session_id, "test-123");
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].msg_type, "user");
        assert_eq!(session.messages[1].msg_type, "gemini");
        assert_eq!(session.messages[1].model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(session.messages[1].thoughts.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_error_message() {
        let json = r#"{
            "sessionId": "test-err",
            "projectHash": "abc",
            "startTime": "2026-02-09T15:10:00.000Z",
            "lastUpdated": "2026-02-09T15:10:30.000Z",
            "messages": [
                {
                    "id": "m1",
                    "timestamp": "2026-02-09T15:10:00.000Z",
                    "type": "user",
                    "content": "test"
                },
                {
                    "id": "m2",
                    "timestamp": "2026-02-09T15:10:30.000Z",
                    "type": "error",
                    "content": "[API Error: No capacity]"
                }
            ]
        }"#;

        let session: GeminiSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.messages[1].msg_type, "error");
    }

    #[test]
    fn test_parse_jsonl_records() {
        // Test that GeminiRecord types deserialize correctly
        let metadata_line = r#"{"type":"session_metadata","sessionId":"sess-1","startTime":"2026-02-09T15:00:00.000Z"}"#;
        let record: GeminiRecord = serde_json::from_str(metadata_line).unwrap();
        match record {
            GeminiRecord::SessionMetadata {
                session_id,
                start_time,
            } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(start_time.as_deref(), Some("2026-02-09T15:00:00.000Z"));
            }
            _ => panic!("Expected SessionMetadata"),
        }

        let user_line = r#"{"type":"user","id":"u1","timestamp":"2026-02-09T15:01:00.000Z","content":[{"type":"text","text":"hello gemini"}]}"#;
        let record: GeminiRecord = serde_json::from_str(user_line).unwrap();
        match record {
            GeminiRecord::User { id, content, .. } => {
                assert_eq!(id.as_deref(), Some("u1"));
                assert_eq!(content.len(), 1);
                match &content[0] {
                    GeminiContentBlock::Text { text } => assert_eq!(text, "hello gemini"),
                    _ => panic!("Expected Text block"),
                }
            }
            _ => panic!("Expected User"),
        }

        let gemini_line = r#"{"type":"gemini","id":"g1","timestamp":"2026-02-09T15:02:00.000Z","content":[{"type":"thinking","text":"analyzing..."},{"type":"text","text":"Here is my answer"},{"type":"functionCall","name":"readFile","args":{"path":"/tmp/x.rs"}}],"model":"gemini-2.5-pro"}"#;
        let record: GeminiRecord = serde_json::from_str(gemini_line).unwrap();
        match record {
            GeminiRecord::Gemini {
                id, content, model, ..
            } => {
                assert_eq!(id.as_deref(), Some("g1"));
                assert_eq!(model.as_deref(), Some("gemini-2.5-pro"));
                assert_eq!(content.len(), 3);
                assert!(matches!(&content[0], GeminiContentBlock::Thinking { .. }));
                assert!(matches!(&content[1], GeminiContentBlock::Text { .. }));
                assert!(matches!(
                    &content[2],
                    GeminiContentBlock::FunctionCall { .. }
                ));
            }
            _ => panic!("Expected Gemini"),
        }

        let update_line =
            r#"{"type":"message_update","id":"g1","tokens":{"input":100,"output":50,"total":150}}"#;
        let record: GeminiRecord = serde_json::from_str(update_line).unwrap();
        match record {
            GeminiRecord::MessageUpdate { id, tokens } => {
                assert_eq!(id.as_deref(), Some("g1"));
                let tokens = tokens.unwrap();
                assert_eq!(tokens.input, Some(100));
                assert_eq!(tokens.output, Some(50));
            }
            _ => panic!("Expected MessageUpdate"),
        }
    }

    #[test]
    fn test_parse_jsonl_function_response() {
        let user_with_response = r#"{"type":"user","id":"u2","timestamp":"2026-02-09T15:03:00.000Z","content":[{"type":"functionResponse","name":"readFile","response":{"content":"fn main() {}"}}]}"#;
        let record: GeminiRecord = serde_json::from_str(user_with_response).unwrap();
        match record {
            GeminiRecord::User { content, .. } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    GeminiContentBlock::FunctionResponse { name, response } => {
                        assert_eq!(name, "readFile");
                        assert!(response.is_some());
                    }
                    _ => panic!("Expected FunctionResponse block"),
                }
            }
            _ => panic!("Expected User"),
        }
    }

    #[test]
    fn test_parse_jsonl_unknown_content_block() {
        // Unknown content block types should be deserialized without error
        let gemini_line = r#"{"type":"gemini","id":"g2","content":[{"type":"unknownFutureType","data":"something"}]}"#;
        let record: GeminiRecord = serde_json::from_str(gemini_line).unwrap();
        match record {
            GeminiRecord::Gemini { content, .. } => {
                assert_eq!(content.len(), 1);
                assert!(matches!(content[0], GeminiContentBlock::Unknown));
            }
            _ => panic!("Expected Gemini"),
        }
    }

    #[test]
    fn test_info_message_skipped() {
        let json = r#"{
            "sessionId": "test-info",
            "projectHash": "abc",
            "startTime": "2026-02-09T15:10:00.000Z",
            "lastUpdated": "2026-02-09T15:10:00.000Z",
            "messages": [
                {
                    "id": "m1",
                    "timestamp": "2026-02-09T15:10:00.000Z",
                    "type": "info",
                    "content": "Authentication succeeded"
                }
            ]
        }"#;

        // info messages should deserialize but produce no events
        let session: GeminiSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].msg_type, "info");
    }
}
