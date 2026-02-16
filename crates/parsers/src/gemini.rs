use crate::common::{
    attach_semantic_attrs, attach_source_attrs, infer_tool_kind, normalize_role_label, set_first,
};
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
    content: Option<GeminiMessageContent>,
    #[serde(default)]
    thoughts: Option<Vec<GeminiThought>>,
    #[allow(dead_code)]
    #[serde(default)]
    tokens: Option<GeminiTokens>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "toolCalls")]
    tool_calls: Vec<GeminiToolCall>,
}

/// Older Gemini JSON sessions may store `message.content` either as plain text
/// or as structured part objects.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GeminiMessageContent {
    Text(String),
    Parts(Vec<serde_json::Value>),
    Part(serde_json::Value),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolCall {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
    #[serde(default)]
    result: Option<GeminiPartListUnion>,
    #[serde(default)]
    status: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    display_name: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    description: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    render_output_as_markdown: Option<bool>,
    #[allow(dead_code)]
    #[serde(default)]
    result_display: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GeminiPartListUnion {
    Text(String),
    Parts(Vec<serde_json::Value>),
    Part(serde_json::Value),
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

#[derive(Debug, Default)]
struct ParsedLegacyContent {
    texts: Vec<String>,
    thinkings: Vec<String>,
    tool_calls: Vec<(String, Option<serde_json::Value>)>,
    tool_results: Vec<(String, Option<serde_json::Value>)>,
    schema_variant: &'static str,
}

fn parse_legacy_part(parsed: &mut ParsedLegacyContent, part: &serde_json::Value) {
    let Some(obj) = part.as_object() else {
        if let Some(text) = part.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parsed.texts.push(trimmed.to_string());
            }
        }
        return;
    };

    if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parsed.texts.push(trimmed.to_string());
        }
    }

    if let Some(thinking) = obj.get("thinking").and_then(|v| v.as_str()) {
        let trimmed = thinking.trim();
        if !trimmed.is_empty() {
            parsed.thinkings.push(trimmed.to_string());
        }
    }

    if let Some(call) = obj.get("functionCall").or_else(|| obj.get("function_call")) {
        if let Some(call_obj) = call.as_object() {
            let name = call_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = call_obj
                .get("args")
                .cloned()
                .or_else(|| call_obj.get("arguments").cloned());
            parsed.tool_calls.push((name, args));
        }
    }

    if let Some(resp) = obj
        .get("functionResponse")
        .or_else(|| obj.get("function_response"))
    {
        if let Some(resp_obj) = resp.as_object() {
            let name = resp_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let response = resp_obj
                .get("response")
                .cloned()
                .or_else(|| resp_obj.get("result").cloned());
            parsed.tool_results.push((name, response));
        }
    }
}

fn parse_legacy_content(content: Option<&GeminiMessageContent>) -> ParsedLegacyContent {
    let Some(content) = content else {
        return ParsedLegacyContent::default();
    };

    match content {
        GeminiMessageContent::Text(text) => ParsedLegacyContent {
            texts: vec![text.clone()],
            schema_variant: "text",
            ..ParsedLegacyContent::default()
        },
        GeminiMessageContent::Part(part) => {
            let mut parsed = ParsedLegacyContent {
                schema_variant: "part",
                ..ParsedLegacyContent::default()
            };
            parse_legacy_part(&mut parsed, part);
            parsed
        }
        GeminiMessageContent::Parts(parts) => {
            let mut parsed = ParsedLegacyContent {
                schema_variant: "parts",
                ..ParsedLegacyContent::default()
            };
            for part in parts {
                parse_legacy_part(&mut parsed, part);
            }
            parsed
        }
    }
}

fn content_from_part_values(parts: &[serde_json::Value]) -> Content {
    if parts.is_empty() {
        return Content::empty();
    }

    let mut text_chunks: Vec<String> = Vec::new();
    let mut response_payloads: Vec<serde_json::Value> = Vec::new();

    for part in parts {
        if let Some(text) = part.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                text_chunks.push(trimmed.to_string());
            }
            continue;
        }

        if let Some(obj) = part.as_object() {
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    text_chunks.push(trimmed.to_string());
                }
            }

            if let Some(resp) = obj
                .get("functionResponse")
                .or_else(|| obj.get("function_response"))
            {
                if let Some(resp_obj) = resp.as_object() {
                    let payload = resp_obj
                        .get("response")
                        .cloned()
                        .or_else(|| resp_obj.get("result").cloned())
                        .unwrap_or_else(|| resp.clone());
                    response_payloads.push(payload);
                } else {
                    response_payloads.push(resp.clone());
                }
                continue;
            }
        }
    }

    if !response_payloads.is_empty() {
        let data = if response_payloads.len() == 1 {
            response_payloads.into_iter().next().unwrap_or_default()
        } else {
            serde_json::Value::Array(response_payloads)
        };
        return Content {
            blocks: vec![ContentBlock::Json { data }],
        };
    }

    if !text_chunks.is_empty() {
        return Content::text(text_chunks.join("\n"));
    }

    let fallback = if parts.len() == 1 {
        parts[0].clone()
    } else {
        serde_json::Value::Array(parts.to_vec())
    };
    Content {
        blocks: vec![ContentBlock::Json { data: fallback }],
    }
}

fn content_from_part_list_union(result: Option<&GeminiPartListUnion>) -> Content {
    match result {
        None => Content::empty(),
        Some(GeminiPartListUnion::Text(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Content::empty()
            } else {
                Content::text(trimmed)
            }
        }
        Some(GeminiPartListUnion::Part(part)) => {
            content_from_part_values(std::slice::from_ref(part))
        }
        Some(GeminiPartListUnion::Parts(parts)) => content_from_part_values(parts),
    }
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

        let parsed = parse_legacy_content(msg.content.as_ref());
        let content_text = parsed
            .texts
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let schema_version = if !msg.tool_calls.is_empty() {
            "gemini-json-v3-toolcalls"
        } else if matches!(parsed.schema_variant, "parts" | "part") {
            "gemini-json-v2-parts"
        } else {
            "gemini-json-v1"
        };

        let mut base_attrs = HashMap::new();
        attach_source_attrs(&mut base_attrs, Some(schema_version), Some(&msg.msg_type));
        if let Some(role) = normalize_role_label(&msg.msg_type) {
            base_attrs.insert(
                "semantic.role".to_string(),
                serde_json::Value::String(role.to_string()),
            );
        }
        if let Some(group_id) = msg.id.as_deref() {
            attach_semantic_attrs(&mut base_attrs, Some(group_id), None, None);
        }

        match msg.msg_type.as_str() {
            "user" => {
                if !content_text.is_empty() {
                    set_first(&mut first_user_text, Some(content_text.clone()));
                    event_counter += 1;
                    events.push(Event {
                        event_id: msg
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("gemini-{}", event_counter)),
                        timestamp: ts,
                        event_type: EventType::UserMessage,
                        task_id: None,
                        content: Content::text(content_text),
                        duration_ms: None,
                        attributes: base_attrs.clone(),
                    });
                }

                for (idx, (name, response)) in parsed.tool_results.iter().enumerate() {
                    event_counter += 1;
                    let call_id = msg.id.as_deref().map(|id| format!("{id}-call-{}", idx + 1));
                    let mut attrs = base_attrs.clone();
                    attach_semantic_attrs(
                        &mut attrs,
                        msg.id.as_deref(),
                        call_id.as_deref(),
                        Some(infer_tool_kind(name)),
                    );
                    let result_content = match response {
                        Some(v) => Content {
                            blocks: vec![ContentBlock::Json { data: v.clone() }],
                        },
                        None => Content::empty(),
                    };
                    events.push(Event {
                        event_id: format!("gemini-{}", event_counter),
                        timestamp: ts,
                        event_type: EventType::ToolResult {
                            name: name.clone(),
                            is_error: false,
                            call_id,
                        },
                        task_id: None,
                        content: result_content,
                        duration_ms: None,
                        attributes: attrs,
                    });
                }
            }
            "gemini" => {
                set_first(&mut model_name, msg.model.clone());

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
                            content: Content::text(text),
                            duration_ms: None,
                            attributes: base_attrs.clone(),
                        });
                    }
                }

                for thinking in &parsed.thinkings {
                    event_counter += 1;
                    events.push(Event {
                        event_id: format!("gemini-{}", event_counter),
                        timestamp: ts,
                        event_type: EventType::Thinking,
                        task_id: None,
                        content: Content::text(thinking),
                        duration_ms: None,
                        attributes: base_attrs.clone(),
                    });
                }

                if !msg.tool_calls.is_empty() {
                    for (idx, tool_call) in msg.tool_calls.iter().enumerate() {
                        let call_id = tool_call.id.clone().or_else(|| {
                            msg.id.as_deref().map(|id| format!("{id}-call-{}", idx + 1))
                        });

                        event_counter += 1;
                        let mut call_attrs = base_attrs.clone();
                        attach_semantic_attrs(
                            &mut call_attrs,
                            msg.id.as_deref(),
                            call_id.as_deref(),
                            Some(infer_tool_kind(&tool_call.name)),
                        );
                        let call_content = match &tool_call.args {
                            Some(v) => Content {
                                blocks: vec![ContentBlock::Json { data: v.clone() }],
                            },
                            None => Content::empty(),
                        };
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolCall {
                                name: tool_call.name.clone(),
                            },
                            task_id: None,
                            content: call_content,
                            duration_ms: None,
                            attributes: call_attrs,
                        });

                        event_counter += 1;
                        let mut result_attrs = base_attrs.clone();
                        attach_semantic_attrs(
                            &mut result_attrs,
                            msg.id.as_deref(),
                            call_id.as_deref(),
                            Some(infer_tool_kind(&tool_call.name)),
                        );
                        let result_content =
                            content_from_part_list_union(tool_call.result.as_ref());
                        let is_error = tool_call
                            .status
                            .as_deref()
                            .is_some_and(|status| status != "success");
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: tool_call.name.clone(),
                                is_error,
                                call_id,
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: result_attrs,
                        });
                    }
                } else {
                    for (idx, (name, args)) in parsed.tool_calls.iter().enumerate() {
                        event_counter += 1;
                        let call_id = msg.id.as_deref().map(|id| format!("{id}-call-{}", idx + 1));
                        let mut attrs = base_attrs.clone();
                        attach_semantic_attrs(
                            &mut attrs,
                            msg.id.as_deref(),
                            call_id.as_deref(),
                            Some(infer_tool_kind(name)),
                        );
                        let call_content = match args {
                            Some(v) => Content {
                                blocks: vec![ContentBlock::Json { data: v.clone() }],
                            },
                            None => Content::empty(),
                        };
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolCall { name: name.clone() },
                            task_id: None,
                            content: call_content,
                            duration_ms: None,
                            attributes: attrs,
                        });
                    }

                    for (idx, (name, response)) in parsed.tool_results.iter().enumerate() {
                        event_counter += 1;
                        let call_id = msg.id.as_deref().map(|id| format!("{id}-call-{}", idx + 1));
                        let mut attrs = base_attrs.clone();
                        attach_semantic_attrs(
                            &mut attrs,
                            msg.id.as_deref(),
                            call_id.as_deref(),
                            Some(infer_tool_kind(name)),
                        );
                        let result_content = match response {
                            Some(v) => Content {
                                blocks: vec![ContentBlock::Json { data: v.clone() }],
                            },
                            None => Content::empty(),
                        };
                        events.push(Event {
                            event_id: format!("gemini-{}", event_counter),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: name.clone(),
                                is_error: false,
                                call_id,
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: attrs,
                        });
                    }
                }

                if !content_text.is_empty() {
                    event_counter += 1;
                    let mut attrs = base_attrs.clone();
                    attrs.extend(token_attrs);
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
            "error" => {
                if !content_text.is_empty() {
                    event_counter += 1;
                    let mut attrs = base_attrs.clone();
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
                let mut base_attrs = HashMap::new();
                attach_source_attrs(&mut base_attrs, Some("gemini-jsonl-v1"), Some("user"));
                if let Some(group_id) = id.as_deref() {
                    attach_semantic_attrs(&mut base_attrs, Some(group_id), None, None);
                }

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
                        attributes: base_attrs.clone(),
                    });
                }

                // Handle functionResponse blocks in user messages (tool results)
                let mut response_idx = 0usize;
                for block in &content {
                    if let GeminiContentBlock::FunctionResponse { name, response } = block {
                        response_idx += 1;
                        event_counter += 1;
                        let call_id = id
                            .as_deref()
                            .map(|gid| format!("{gid}-call-{response_idx}"));
                        let mut attrs = base_attrs.clone();
                        attach_semantic_attrs(
                            &mut attrs,
                            id.as_deref(),
                            call_id.as_deref(),
                            Some(infer_tool_kind(name)),
                        );
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
                                call_id,
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: attrs,
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
                let mut base_attrs = HashMap::new();
                attach_source_attrs(&mut base_attrs, Some("gemini-jsonl-v1"), Some("gemini"));
                if let Some(group_id) = id.as_deref() {
                    attach_semantic_attrs(&mut base_attrs, Some(group_id), None, None);
                }

                let event_base_id =
                    id.unwrap_or_else(|| format!("gemini-auto-{}", event_counter + 1));

                let mut call_idx = 0usize;
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
                                    attributes: base_attrs.clone(),
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
                                    attributes: base_attrs.clone(),
                                });
                            }
                        }
                        GeminiContentBlock::FunctionCall { name, args } => {
                            event_counter += 1;
                            call_idx += 1;
                            let call_id = Some(format!("{event_base_id}-call-{call_idx}"));
                            let mut attrs = base_attrs.clone();
                            attach_semantic_attrs(
                                &mut attrs,
                                Some(&event_base_id),
                                call_id.as_deref(),
                                Some(infer_tool_kind(name)),
                            );
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
                                attributes: attrs,
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
    use std::fs::write;

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

    #[test]
    fn test_parse_legacy_content_parts_variant() {
        let content = GeminiMessageContent::Parts(vec![serde_json::json!({
            "text": "hello from parts"
        })]);
        let parsed = parse_legacy_content(Some(&content));
        assert_eq!(parsed.schema_variant, "parts");
        assert_eq!(parsed.texts, vec!["hello from parts".to_string()]);
    }

    #[test]
    fn test_parse_legacy_content_single_part_variant() {
        let content = GeminiMessageContent::Part(serde_json::json!({
            "functionCall": {
                "name": "read_file",
                "args": {"path": "/tmp/a.txt"}
            }
        }));
        let parsed = parse_legacy_content(Some(&content));
        assert_eq!(parsed.schema_variant, "part");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].0, "read_file");
    }

    #[test]
    fn test_parse_json_tool_calls_field() {
        let dir = std::env::temp_dir().join(format!(
            "opensession-gemini-toolcalls-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session-2026-02-16T09-10-toolcalls.json");
        let json = r#"{
            "sessionId": "toolcalls-123",
            "projectHash": "abc",
            "startTime": "2026-02-16T09:10:00.000Z",
            "lastUpdated": "2026-02-16T09:10:10.000Z",
            "messages": [
                {
                    "id": "u1",
                    "timestamp": "2026-02-16T09:10:01.000Z",
                    "type": "user",
                    "content": [{"text":"version"}]
                },
                {
                    "id": "g1",
                    "timestamp": "2026-02-16T09:10:03.000Z",
                    "type": "gemini",
                    "content": "running tool",
                    "model": "gemini-2.5-flash",
                    "toolCalls": [
                        {
                            "id": "call-1",
                            "name": "run_shell_command",
                            "args": {"command":"git status"},
                            "result": [
                                {
                                    "functionResponse": {
                                        "id": "call-1",
                                        "name": "run_shell_command",
                                        "response": {"output":"clean"}
                                    }
                                }
                            ],
                            "status": "success"
                        }
                    ],
                    "tokens": {"input": 11, "output": 7, "total": 18}
                }
            ]
        }"#;
        write(&path, json).unwrap();

        let parsed = parse_json(&path).expect("parse gemini json with toolCalls");
        assert!(parsed.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::ToolCall { name } if name == "run_shell_command"
            )
        }));
        assert!(parsed.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::ToolResult { name, call_id, is_error }
                    if name == "run_shell_command"
                        && call_id.as_deref() == Some("call-1")
                        && !is_error
            )
        }));
        assert!(parsed.events.iter().any(|event| {
            event
                .attributes
                .get("source.schema_version")
                .and_then(|v| v.as_str())
                == Some("gemini-json-v3-toolcalls")
        }));
    }

    #[test]
    fn test_parse_json_parts_content_file() {
        let dir = std::env::temp_dir().join(format!(
            "opensession-gemini-parts-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session-2026-02-14T09-36-test.json");
        let json = r#"{
            "sessionId": "parts-123",
            "startTime": "2026-02-14T09:36:00.000Z",
            "lastUpdated": "2026-02-14T09:36:05.000Z",
            "messages": [
                {
                    "id": "u1",
                    "timestamp": "2026-02-14T09:36:01.000Z",
                    "type": "user",
                    "content": [{"text":"inspect this repo"}]
                },
                {
                    "id": "g1",
                    "timestamp": "2026-02-14T09:36:03.000Z",
                    "type": "gemini",
                    "content": [{"text":"done"}],
                    "model": "gemini-2.5-pro"
                }
            ]
        }"#;
        write(&path, json).unwrap();

        let parsed = parse_json(&path).expect("parse gemini json");
        assert_eq!(parsed.session_id, "parts-123");
        assert!(parsed
            .events
            .iter()
            .any(|e| matches!(e.event_type, EventType::UserMessage)));
        assert!(parsed
            .events
            .iter()
            .any(|e| matches!(e.event_type, EventType::AgentMessage)));
        assert!(parsed.events.iter().all(|e| {
            e.attributes
                .get("source.schema_version")
                .and_then(|v| v.as_str())
                .is_some()
        }));
    }
}
