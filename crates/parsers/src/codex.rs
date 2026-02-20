use crate::common::{attach_semantic_attrs, attach_source_attrs, infer_tool_kind, set_first};
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Default)]
struct RequestUserInputCallMeta {
    questions: Vec<InteractiveQuestionMeta>,
}

#[derive(Debug, Clone, Default)]
struct InteractiveQuestionMeta {
    id: String,
    header: Option<String>,
    question: Option<String>,
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
    let mut open_tasks: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut interactive_call_meta: HashMap<String, RequestUserInputCallMeta> = HashMap::new();

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
                process_item_with_options(
                    payload,
                    entry_ts,
                    &mut events,
                    &mut event_counter,
                    user_text_target,
                    &mut last_function_name,
                    &mut call_map,
                    &mut interactive_call_meta,
                    is_desktop,
                );
            }
            continue;
        }

        // Codex Desktop: `event_msg` contains UI-level events
        if top_type == "event_msg" {
            if let Some(payload) = obj.get("payload") {
                let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match payload_type {
                    "user_message" => {
                        if let Some(msg) = payload.get("message").and_then(|v| v.as_str()) {
                            let text = msg.trim().to_string();
                            if text.is_empty() || looks_like_injected_codex_user_text(&text) {
                                continue;
                            }
                            set_first(&mut first_user_text, Some(text.clone()));
                            push_user_message_event(
                                &mut events,
                                &mut event_counter,
                                entry_ts,
                                &text,
                                Some("event_msg"),
                            );
                        }
                    }
                    "agent_message" => {
                        if let Some(msg) = payload
                            .get("message")
                            .or_else(|| payload.get("text"))
                            .or_else(|| payload.get("content"))
                            .and_then(|v| v.as_str())
                        {
                            push_agent_message_event(
                                &mut events,
                                &mut event_counter,
                                entry_ts,
                                msg,
                                Some("event_msg"),
                            );
                        }
                    }
                    "agent_reasoning" | "agent_reasoning_raw_content" => {
                        if let Some(reasoning) = payload
                            .get("message")
                            .or_else(|| payload.get("text"))
                            .or_else(|| payload.get("content"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                        {
                            event_counter += 1;
                            let mut attributes = HashMap::new();
                            let raw_type = if payload_type == "agent_reasoning_raw_content" {
                                "event_msg:agent_reasoning_raw_content"
                            } else {
                                "event_msg:agent_reasoning"
                            };
                            attach_source_attrs(
                                &mut attributes,
                                Some("codex-desktop-v1"),
                                Some(raw_type),
                            );
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: entry_ts,
                                event_type: EventType::Thinking,
                                task_id: None,
                                content: Content::text(reasoning),
                                duration_ms: None,
                                attributes,
                            });
                        }
                    }
                    "token_count" => {
                        let sampled = extract_token_counts(payload);
                        let cumulative = extract_total_token_counts(payload);
                        if sampled.is_some() || cumulative.is_some() {
                            event_counter += 1;
                            let mut attributes = HashMap::new();
                            attach_source_attrs(
                                &mut attributes,
                                Some("codex-desktop-v1"),
                                Some("event_msg:token_count"),
                            );
                            if let Some((input_tokens, output_tokens)) = sampled {
                                if let Some(input_tokens) = input_tokens {
                                    attributes.insert(
                                        "input_tokens".to_string(),
                                        serde_json::Value::Number(input_tokens.into()),
                                    );
                                }
                                if let Some(output_tokens) = output_tokens {
                                    attributes.insert(
                                        "output_tokens".to_string(),
                                        serde_json::Value::Number(output_tokens.into()),
                                    );
                                }
                            }
                            if let Some((input_total_tokens, output_total_tokens)) = cumulative {
                                if let Some(input_total_tokens) = input_total_tokens {
                                    attributes.insert(
                                        "input_tokens_total".to_string(),
                                        serde_json::Value::Number(input_total_tokens.into()),
                                    );
                                }
                                if let Some(output_total_tokens) = output_total_tokens {
                                    attributes.insert(
                                        "output_tokens_total".to_string(),
                                        serde_json::Value::Number(output_total_tokens.into()),
                                    );
                                }
                            }
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: entry_ts,
                                event_type: EventType::Custom {
                                    kind: "token_count".to_string(),
                                },
                                task_id: payload
                                    .get("turn_id")
                                    .or_else(|| payload.get("task_id"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string),
                                content: Content::empty(),
                                duration_ms: None,
                                attributes,
                            });
                        }
                    }
                    "context_compacted" => {
                        event_counter += 1;
                        let mut attributes = HashMap::new();
                        attach_source_attrs(
                            &mut attributes,
                            Some("codex-desktop-v1"),
                            Some("event_msg:context_compacted"),
                        );
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: entry_ts,
                            event_type: EventType::Custom {
                                kind: "context_compacted".to_string(),
                            },
                            task_id: payload
                                .get("turn_id")
                                .or_else(|| payload.get("task_id"))
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            content: Content::text("context compacted"),
                            duration_ms: None,
                            attributes,
                        });
                    }
                    "item_completed" => {
                        let item = payload.get("item").unwrap_or(&serde_json::Value::Null);
                        let item_type = item
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .unwrap_or("");
                        if item_type.eq_ignore_ascii_case("plan") {
                            event_counter += 1;
                            let mut attributes = HashMap::new();
                            attach_source_attrs(
                                &mut attributes,
                                Some("codex-desktop-v1"),
                                Some("event_msg:item_completed"),
                            );
                            if let Some(plan_id) = item.get("id").and_then(|v| v.as_str()) {
                                attributes.insert(
                                    "plan_id".to_string(),
                                    serde_json::Value::String(plan_id.to_string()),
                                );
                            }
                            if let Some(turn_id) = payload.get("turn_id").and_then(|v| v.as_str()) {
                                attributes.insert(
                                    "turn_id".to_string(),
                                    serde_json::Value::String(turn_id.to_string()),
                                );
                            }
                            let plan_preview = item
                                .get("text")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .and_then(|v| v.lines().find(|line| !line.trim().is_empty()))
                                .map(str::trim)
                                .unwrap_or("plan completed");
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: entry_ts,
                                event_type: EventType::Custom {
                                    kind: "plan_completed".to_string(),
                                },
                                task_id: payload
                                    .get("turn_id")
                                    .or_else(|| payload.get("task_id"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string),
                                content: Content::text(format!("Plan completed: {plan_preview}")),
                                duration_ms: None,
                                attributes,
                            });
                        }
                    }
                    "turn_aborted" => {
                        event_counter += 1;
                        let mut attributes = HashMap::new();
                        if let Some(reason) = payload
                            .get("reason")
                            .or_else(|| payload.get("message"))
                            .or_else(|| payload.get("error"))
                            .and_then(|v| v.as_str())
                        {
                            attributes.insert(
                                "reason".to_string(),
                                serde_json::Value::String(reason.to_string()),
                            );
                        }
                        let task_id = payload
                            .get("turn_id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        events.push(Event {
                            event_id: format!("codex-{}", event_counter),
                            timestamp: entry_ts,
                            event_type: EventType::Custom {
                                kind: "turn_aborted".to_string(),
                            },
                            task_id,
                            content: Content::text("turn aborted"),
                            duration_ms: None,
                            attributes,
                        });
                    }
                    "task_started" => {
                        let turn_id = payload
                            .get("turn_id")
                            .or_else(|| payload.get("task_id"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(String::from);
                        if let Some(task_id) = turn_id {
                            let title = payload
                                .get("title")
                                .or_else(|| payload.get("task"))
                                .or_else(|| payload.get("name"))
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(String::from);
                            open_tasks.insert(task_id.clone(), title.clone());
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: entry_ts,
                                event_type: EventType::TaskStart {
                                    title: title.clone(),
                                },
                                task_id: Some(task_id),
                                content: Content::text(
                                    title.unwrap_or_else(|| "task started".to_string()),
                                ),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    "task_complete" | "task_completed" | "task_finished" => {
                        let turn_id = payload
                            .get("turn_id")
                            .or_else(|| payload.get("task_id"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(String::from);
                        if let Some(task_id) = turn_id {
                            let summary = payload
                                .get("last_agent_message")
                                .or_else(|| payload.get("summary"))
                                .or_else(|| payload.get("message"))
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(String::from);
                            if let Some(summary_text) = summary.as_deref() {
                                push_agent_message_event(
                                    &mut events,
                                    &mut event_counter,
                                    entry_ts,
                                    summary_text,
                                    Some("event_msg"),
                                );
                            }
                            open_tasks.remove(&task_id);
                            event_counter += 1;
                            events.push(Event {
                                event_id: format!("codex-{}", event_counter),
                                timestamp: entry_ts,
                                event_type: EventType::TaskEnd {
                                    summary: summary.clone(),
                                },
                                task_id: Some(task_id),
                                content: Content::text(
                                    summary.unwrap_or_else(|| "task completed".to_string()),
                                ),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        // Skip other Desktop-only wrapper types
        if top_type == "turn_context" {
            continue;
        }

        // Legacy flat entry with type field (message, reasoning, function_call, etc.)
        process_item_with_options(
            &v,
            entry_ts,
            &mut events,
            &mut event_counter,
            &mut first_user_text,
            &mut last_function_name,
            &mut call_map,
            &mut interactive_call_meta,
            is_desktop,
        );
    }

    if !open_tasks.is_empty() {
        let synthetic_ts = events
            .last()
            .map(|event| event.timestamp)
            .or(session_ts)
            .unwrap_or_else(Utc::now);
        for (task_id, title) in open_tasks {
            event_counter += 1;
            events.push(Event {
                event_id: format!("codex-{}", event_counter),
                timestamp: synthetic_ts,
                event_type: EventType::TaskEnd {
                    summary: Some("synthetic end (missing task_complete)".to_string()),
                },
                task_id: Some(task_id),
                content: Content::text(title.unwrap_or_else(|| "synthetic task end".to_string())),
                duration_ms: None,
                attributes: HashMap::new(),
            });
        }
    }

    // ── Build Session ───────────────────────────────────────────────────────

    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let (provider, model) = load_codex_agent_identity();
    let agent = Agent {
        provider,
        model,
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
        if let Some(branch) = json_object_string(
            &git,
            &["branch", "git_branch", "current_branch", "ref", "head"],
        ) {
            attributes.insert("git_branch".to_string(), serde_json::Value::String(branch));
        }
        if let Some(repo_name) =
            json_object_string(&git, &["repo_name", "repository", "repo", "name"])
        {
            attributes.insert(
                "git_repo_name".to_string(),
                serde_json::Value::String(repo_name),
            );
        }
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
#[cfg(test)]
fn process_item(
    item: &serde_json::Value,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    counter: &mut u64,
    first_user_text: &mut Option<String>,
    last_function_name: &mut String,
    call_map: &mut HashMap<String, (String, String)>,
) {
    let mut interactive_call_meta = HashMap::new();
    process_item_with_options(
        item,
        ts,
        events,
        counter,
        first_user_text,
        last_function_name,
        call_map,
        &mut interactive_call_meta,
        false,
    );
}

#[allow(clippy::too_many_arguments)]
fn process_item_with_options(
    item: &serde_json::Value,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    counter: &mut u64,
    first_user_text: &mut Option<String>,
    last_function_name: &mut String,
    call_map: &mut HashMap<String, (String, String)>,
    interactive_call_meta: &mut HashMap<String, RequestUserInputCallMeta>,
    filter_injected_user_text: bool,
) {
    let item_type = match item.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return,
    };

    match item_type {
        "message" => {
            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let text = extract_message_text_blocks(item.get("content"));

            if text.is_empty() {
                return;
            }

            if role == "user"
                && filter_injected_user_text
                && looks_like_injected_codex_user_text(&text)
            {
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

            if matches!(event_type, EventType::UserMessage) {
                let source = if filter_injected_user_text {
                    Some("response_fallback")
                } else {
                    None
                };
                push_user_message_event(events, counter, ts, &text, source);
            } else {
                let source = if filter_injected_user_text {
                    Some("response_fallback")
                } else {
                    None
                };
                push_agent_message_event(events, counter, ts, &text, source);
            }
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
                let mut attributes = HashMap::new();
                attach_source_attrs(&mut attributes, Some("codex-jsonl-v1"), Some("reasoning"));
                events.push(Event {
                    event_id: format!("codex-{}", counter),
                    timestamp: ts,
                    event_type: EventType::Thinking,
                    task_id: None,
                    content: Content::text(&text),
                    duration_ms: None,
                    attributes,
                });
            }
        }
        "function_call" | "custom_tool_call" => {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let custom_input = item.get("input").and_then(|v| v.as_str()).unwrap_or("");
            // function_call: arguments is a JSON string
            // custom_tool_call: input is a raw string (patch content, etc.)
            let args: serde_json::Value = if item_type == "custom_tool_call" {
                serde_json::json!({ "input": custom_input })
            } else {
                let args_str = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                serde_json::from_str(args_str).unwrap_or(serde_json::Value::Null)
            };

            let call_id = item
                .get("call_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if name == "request_user_input" {
                if let Some(call_id) = call_id.as_ref() {
                    let meta = parse_request_user_input_call_meta(&args);
                    if !meta.questions.is_empty() {
                        interactive_call_meta.insert(call_id.clone(), meta);
                    }
                }
            }

            let event_type = classify_codex_function(&name, &args);
            let content = if item_type == "custom_tool_call" {
                // Custom tools store input as raw text (e.g. patch content)
                Content::text(custom_input)
            } else {
                codex_function_content(&name, &args)
            };

            *counter += 1;
            let event_id = format!("codex-{}", counter);
            let mut attributes = HashMap::new();
            attach_source_attrs(
                &mut attributes,
                Some("codex-jsonl-v1"),
                Some(if item_type == "custom_tool_call" {
                    "custom_tool_call"
                } else {
                    "function_call"
                }),
            );
            attach_semantic_attrs(
                &mut attributes,
                None,
                call_id.as_deref(),
                Some(infer_tool_kind(&name)),
            );

            if let Some(call_id) = call_id.as_deref() {
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
                attributes,
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

            if call_name == "request_user_input" {
                let call_meta = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .and_then(|call_id| interactive_call_meta.remove(call_id));
                if let Some((interactive_text, question_ids, raw_answers)) =
                    parse_request_user_input_answers(&output_text)
                {
                    if let Some(meta) = call_meta {
                        if !meta.questions.is_empty() {
                            *counter += 1;
                            let mut attributes = HashMap::new();
                            attributes.insert(
                                "source".to_string(),
                                serde_json::Value::String("interactive_question".to_string()),
                            );
                            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                                attributes.insert(
                                    "call_id".to_string(),
                                    serde_json::Value::String(call_id.to_string()),
                                );
                            }
                            attributes.insert(
                                "question_ids".to_string(),
                                serde_json::Value::Array(
                                    meta.questions
                                        .iter()
                                        .map(|q| serde_json::Value::String(q.id.clone()))
                                        .collect(),
                                ),
                            );
                            attributes.insert(
                                "question_meta".to_string(),
                                serde_json::Value::Array(
                                    meta.questions
                                        .iter()
                                        .map(|q| {
                                            let mut row = serde_json::Map::new();
                                            row.insert(
                                                "id".to_string(),
                                                serde_json::Value::String(q.id.clone()),
                                            );
                                            if let Some(header) = q.header.as_ref() {
                                                row.insert(
                                                    "header".to_string(),
                                                    serde_json::Value::String(header.clone()),
                                                );
                                            }
                                            if let Some(question) = q.question.as_ref() {
                                                row.insert(
                                                    "question".to_string(),
                                                    serde_json::Value::String(question.clone()),
                                                );
                                            }
                                            serde_json::Value::Object(row)
                                        })
                                        .collect(),
                                ),
                            );
                            events.push(Event {
                                event_id: format!("codex-{}", counter),
                                timestamp: ts,
                                event_type: EventType::SystemMessage,
                                task_id: None,
                                content: Content::text(render_interactive_questions(
                                    &meta.questions,
                                )),
                                duration_ms: None,
                                attributes,
                            });
                        }
                    }
                    set_first(first_user_text, Some(interactive_text.clone()));
                    *counter += 1;
                    let mut attributes = HashMap::new();
                    attributes.insert(
                        "source".to_string(),
                        serde_json::Value::String("interactive".to_string()),
                    );
                    attributes.insert(
                        "question_ids".to_string(),
                        serde_json::Value::Array(
                            question_ids
                                .iter()
                                .map(|id| serde_json::Value::String(id.clone()))
                                .collect(),
                        ),
                    );
                    if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                        attributes.insert(
                            "call_id".to_string(),
                            serde_json::Value::String(call_id.to_string()),
                        );
                    }
                    attributes.insert("raw_answers".to_string(), raw_answers);
                    events.push(Event {
                        event_id: format!("codex-{}", counter),
                        timestamp: ts,
                        event_type: EventType::UserMessage,
                        task_id: None,
                        content: Content::text(interactive_text),
                        duration_ms: None,
                        attributes,
                    });
                }
            }

            *counter += 1;
            let semantic_call_id = item.get("call_id").and_then(|v| v.as_str());
            let mut attributes = HashMap::new();
            attach_source_attrs(
                &mut attributes,
                Some("codex-jsonl-v1"),
                Some(if item_type == "custom_tool_call_output" {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                }),
            );
            attach_semantic_attrs(
                &mut attributes,
                None,
                semantic_call_id,
                Some(infer_tool_kind(&call_name)),
            );
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
                attributes,
            });
        }
        "web_search_call" => {
            let action = item.get("action").unwrap_or(&serde_json::Value::Null);
            let action_type = action
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from);
            let semantic_call_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty());
            let mut query_candidates: Vec<String> = action
                .get("queries")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from)
                .collect();
            if let Some(query) = action
                .get("query")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                if !query_candidates.iter().any(|existing| existing == query) {
                    query_candidates.insert(0, query.to_string());
                }
            }
            let url = action
                .get("url")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from);
            let pattern = action
                .get("pattern")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from);

            let web_event = match action_type {
                "search" => {
                    if query_candidates.is_empty() {
                        None
                    } else {
                        let joined = query_candidates.join(" | ");
                        let primary = query_candidates
                            .first()
                            .cloned()
                            .unwrap_or_else(|| joined.clone());
                        Some((
                            EventType::WebSearch { query: primary },
                            Content::text(joined),
                        ))
                    }
                }
                "open_page" | "openPage" => {
                    if let Some(url) = url.clone() {
                        Some((EventType::WebFetch { url: url.clone() }, Content::text(url)))
                    } else {
                        Some((
                            EventType::ToolCall {
                                name: "web_search".to_string(),
                            },
                            Content::text("open_page"),
                        ))
                    }
                }
                "find_in_page" | "findInPage" => {
                    if let Some(url) = url.clone() {
                        let mut details = url.clone();
                        if let Some(pattern) = pattern.as_deref() {
                            details.push_str("\npattern: ");
                            details.push_str(pattern);
                        }
                        Some((EventType::WebFetch { url }, Content::text(details)))
                    } else {
                        pattern.clone().map(|pattern| {
                            (
                                EventType::ToolCall {
                                    name: "web_search".to_string(),
                                },
                                Content::text(format!("find_in_page: {pattern}")),
                            )
                        })
                    }
                }
                _ => {
                    if !query_candidates.is_empty() {
                        let joined = query_candidates.join(" | ");
                        Some((
                            EventType::WebSearch {
                                query: query_candidates
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| joined.clone()),
                            },
                            Content::text(joined),
                        ))
                    } else if let Some(url) = url.clone() {
                        Some((EventType::WebFetch { url: url.clone() }, Content::text(url)))
                    } else {
                        pattern.clone().map(|pattern| {
                            (
                                EventType::ToolCall {
                                    name: "web_search".to_string(),
                                },
                                Content::text(pattern),
                            )
                        })
                    }
                }
            };

            if let Some((event_type, content)) = web_event {
                *counter += 1;
                let mut attributes = HashMap::new();
                let raw_type = if action_type.is_empty() {
                    "web_search_call".to_string()
                } else {
                    format!("web_search_call:{action_type}")
                };
                attach_source_attrs(
                    &mut attributes,
                    Some("codex-jsonl-v1"),
                    Some(raw_type.as_str()),
                );
                attach_semantic_attrs(&mut attributes, None, semantic_call_id, Some("web"));
                if let Some(status) = status {
                    attributes.insert(
                        "web_search.status".to_string(),
                        serde_json::Value::String(status),
                    );
                }
                if !query_candidates.is_empty() {
                    attributes.insert(
                        "web_search.queries".to_string(),
                        serde_json::Value::Array(
                            query_candidates
                                .iter()
                                .map(|query| serde_json::Value::String(query.clone()))
                                .collect(),
                        ),
                    );
                }
                if let Some(pattern) = pattern {
                    attributes.insert(
                        "web_search.pattern".to_string(),
                        serde_json::Value::String(pattern),
                    );
                }
                events.push(Event {
                    event_id: format!("codex-{}", counter),
                    timestamp: ts,
                    event_type,
                    task_id: None,
                    content,
                    duration_ms: None,
                    attributes,
                });
            }
        }
        _ => {}
    }
}

fn push_user_message_event(
    events: &mut Vec<Event>,
    counter: &mut u64,
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if matches!(source, Some("event_msg")) {
        remove_duplicate_response_fallback(events, ts, trimmed);
    }
    if should_skip_duplicate_user_event(events, ts, trimmed, source) {
        return;
    }

    *counter += 1;
    let mut attributes = HashMap::new();
    if let Some(source) = source {
        attributes.insert(
            "source".to_string(),
            serde_json::Value::String(source.to_string()),
        );
        attach_source_attrs(&mut attributes, Some("codex-desktop-v1"), Some(source));
    }
    events.push(Event {
        event_id: format!("codex-{}", counter),
        timestamp: ts,
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text(trimmed),
        duration_ms: None,
        attributes,
    });
}

fn push_agent_message_event(
    events: &mut Vec<Event>,
    counter: &mut u64,
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if matches!(source, Some("event_msg")) {
        remove_duplicate_agent_response_fallback(events, ts, trimmed);
    }
    if should_skip_duplicate_agent_event(events, ts, trimmed, source) {
        return;
    }

    *counter += 1;
    let mut attributes = HashMap::new();
    if let Some(source) = source {
        attributes.insert(
            "source".to_string(),
            serde_json::Value::String(source.to_string()),
        );
        attach_source_attrs(&mut attributes, Some("codex-desktop-v1"), Some(source));
    }
    events.push(Event {
        event_id: format!("codex-{}", counter),
        timestamp: ts,
        event_type: EventType::AgentMessage,
        task_id: None,
        content: Content::text(trimmed),
        duration_ms: None,
        attributes,
    });
}

fn remove_duplicate_response_fallback(events: &mut Vec<Event>, ts: DateTime<Utc>, text: &str) {
    let normalized = normalize_user_text_for_dedupe(text);
    events.retain(|event| {
        if !matches!(event.event_type, EventType::UserMessage) {
            return true;
        }
        if event
            .attributes
            .get("source")
            .and_then(|value| value.as_str())
            != Some("response_fallback")
        {
            return true;
        }
        if (event.timestamp - ts).num_seconds().abs() > 12 {
            return true;
        }
        event_user_text(event)
            .map(|existing| !user_texts_equivalent(&existing, &normalized))
            .unwrap_or(true)
    });
}

fn remove_duplicate_agent_response_fallback(
    events: &mut Vec<Event>,
    ts: DateTime<Utc>,
    text: &str,
) {
    let normalized = normalize_user_text_for_dedupe(text);
    events.retain(|event| {
        if !matches!(event.event_type, EventType::AgentMessage) {
            return true;
        }
        if event
            .attributes
            .get("source")
            .and_then(|value| value.as_str())
            != Some("response_fallback")
        {
            return true;
        }
        if (event.timestamp - ts).num_seconds().abs() > 12 {
            return true;
        }
        event_agent_text(event)
            .map(|existing| !user_texts_equivalent(&existing, &normalized))
            .unwrap_or(true)
    });
}

fn should_skip_duplicate_user_event(
    events: &[Event],
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) -> bool {
    let source = match source {
        Some(source) => source,
        None => return false,
    };
    let opposite = match opposite_dedupe_source(source) {
        Some(opposite) => opposite,
        None => return false,
    };
    let normalized = normalize_user_text_for_dedupe(text);
    events.iter().any(|event| {
        if !matches!(event.event_type, EventType::UserMessage) {
            return false;
        }
        let event_source = event
            .attributes
            .get("source")
            .and_then(|value| value.as_str());
        if event_source != Some(opposite) && event_source != Some(source) {
            return false;
        }
        let duplicate_window_secs = if event_source == Some(source) { 2 } else { 12 };
        if (event.timestamp - ts).num_seconds().abs() > duplicate_window_secs {
            return false;
        }
        event_user_text(event)
            .map(|existing| user_texts_equivalent(&existing, &normalized))
            .unwrap_or(false)
    })
}

fn should_skip_duplicate_agent_event(
    events: &[Event],
    ts: DateTime<Utc>,
    text: &str,
    source: Option<&str>,
) -> bool {
    let source = match source {
        Some(source) => source,
        None => return false,
    };
    let opposite = match opposite_dedupe_source(source) {
        Some(opposite) => opposite,
        None => return false,
    };
    let normalized = normalize_user_text_for_dedupe(text);
    events.iter().any(|event| {
        if !matches!(event.event_type, EventType::AgentMessage) {
            return false;
        }
        let event_source = event
            .attributes
            .get("source")
            .and_then(|value| value.as_str());
        if event_source != Some(opposite) && event_source != Some(source) {
            return false;
        }
        let duplicate_window_secs = if event_source == Some(source) { 2 } else { 12 };
        if (event.timestamp - ts).num_seconds().abs() > duplicate_window_secs {
            return false;
        }
        event_agent_text(event)
            .map(|existing| user_texts_equivalent(&existing, &normalized))
            .unwrap_or(false)
    })
}

fn opposite_dedupe_source(source: &str) -> Option<&'static str> {
    match source {
        "event_msg" => Some("response_fallback"),
        "response_fallback" => Some("event_msg"),
        _ => None,
    }
}

fn event_user_text(event: &Event) -> Option<String> {
    if !matches!(event.event_type, EventType::UserMessage) {
        return None;
    }
    let mut out = Vec::new();
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

fn event_agent_text(event: &Event) -> Option<String> {
    if !matches!(event.event_type, EventType::AgentMessage) {
        return None;
    }
    let mut out = Vec::new();
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

fn normalize_user_text_for_dedupe(text: &str) -> String {
    let normalized = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !matches!(
                lower.as_str(),
                "<image>" | "<file>" | "<audio>" | "<video>" | "[image]" | "[file]"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn user_texts_equivalent(lhs: &str, rhs: &str) -> bool {
    let left = normalize_user_text_for_dedupe(lhs);
    let right = normalize_user_text_for_dedupe(rhs);
    if left == right {
        return true;
    }

    let min_len = left.chars().count().min(right.chars().count());
    min_len >= 16 && (left.contains(&right) || right.contains(&left))
}

#[allow(dead_code)]
fn normalize_user_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn parse_request_user_input_call_meta(args: &serde_json::Value) -> RequestUserInputCallMeta {
    let mut questions = Vec::new();
    let Some(items) = args.get("questions").and_then(|v| v.as_array()) else {
        return RequestUserInputCallMeta { questions };
    };

    for item in items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("question")
            .to_string();
        let header = item
            .get("header")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let question = item
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        questions.push(InteractiveQuestionMeta {
            id,
            header,
            question,
        });
    }

    RequestUserInputCallMeta { questions }
}

fn render_interactive_questions(questions: &[InteractiveQuestionMeta]) -> String {
    let mut lines = Vec::new();
    for q in questions {
        let mut label = q.id.clone();
        if let Some(header) = q.header.as_deref() {
            label = format!("{label} ({header})");
        }
        let body = q.question.as_deref().unwrap_or("(no question text)");
        lines.push(format!("- {label}: {body}"));
    }
    if lines.is_empty() {
        "(no interactive questions)".to_string()
    } else {
        lines.join("\n")
    }
}

fn parse_request_user_input_answers(
    output_text: &str,
) -> Option<(String, Vec<String>, serde_json::Value)> {
    let parsed: serde_json::Value = serde_json::from_str(output_text).ok()?;
    let answers = parsed.get("answers").and_then(|v| v.as_object())?;
    if answers.is_empty() {
        return None;
    }

    let mut question_ids: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (question_id, value) in answers {
        question_ids.push(question_id.clone());
        let mut picks: Vec<String> = Vec::new();
        if let Some(arr) = value.get("answers").and_then(|v| v.as_array()) {
            for answer in arr {
                let rendered = answer
                    .as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        answer
                            .as_object()
                            .and_then(|obj| obj.get("value").and_then(|v| v.as_str()))
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or_else(|| answer.to_string());
                if !rendered.trim().is_empty() {
                    picks.push(rendered);
                }
            }
        } else if let Some(s) = value.as_str() {
            if !s.trim().is_empty() {
                picks.push(s.trim().to_string());
            }
        } else if !value.is_null() {
            picks.push(value.to_string());
        }
        if picks.is_empty() {
            lines.push(format!("{question_id}: (no answer)"));
        } else {
            lines.push(format!("{question_id}: {}", picks.join(" | ")));
        }
    }

    let rendered = lines.join("\n");
    Some((rendered, question_ids, parsed))
}

fn extract_token_counts(payload: &serde_json::Value) -> Option<(Option<u64>, Option<u64>)> {
    let pick = |v: &serde_json::Value, keys: &[&str]| -> Option<u64> {
        for key in keys {
            if let Some(num) = v.get(*key).and_then(|value| value.as_u64()) {
                return Some(num);
            }
            if let Some(num) = v
                .get(*key)
                .and_then(|value| value.as_i64())
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
            {
                return Some(num);
            }
        }
        None
    };

    let usage_pick_input = |value: &serde_json::Value| {
        pick(
            value,
            &[
                "input_tokens",
                "prompt_tokens",
                "inputTokens",
                "promptTokens",
                "token_input",
                "tokenInput",
            ],
        )
    };
    let usage_pick_output = |value: &serde_json::Value| {
        pick(
            value,
            &[
                "output_tokens",
                "completion_tokens",
                "outputTokens",
                "completionTokens",
                "token_output",
                "tokenOutput",
            ],
        )
    };
    fn info_usage<'a>(
        info: &'a serde_json::Value,
        snake_case_key: &str,
        camel_case_key: &str,
    ) -> Option<&'a serde_json::Value> {
        if let Some(value) = info.get(snake_case_key) {
            Some(value)
        } else {
            info.get(camel_case_key)
        }
    }

    let input = pick(
        payload,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
            "token_input",
            "tokenInput",
        ],
    )
    .or_else(|| payload.get("usage").and_then(usage_pick_input))
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "last_token_usage", "lastTokenUsage"))
            .and_then(usage_pick_input)
    })
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "total_token_usage", "totalTokenUsage"))
            .and_then(usage_pick_input)
    });
    let output = pick(
        payload,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
            "token_output",
            "tokenOutput",
        ],
    )
    .or_else(|| payload.get("usage").and_then(usage_pick_output))
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "last_token_usage", "lastTokenUsage"))
            .and_then(usage_pick_output)
    })
    .or_else(|| {
        payload
            .get("info")
            .and_then(|info| info_usage(info, "total_token_usage", "totalTokenUsage"))
            .and_then(usage_pick_output)
    });
    if input.is_none() && output.is_none() {
        None
    } else {
        Some((input, output))
    }
}

fn extract_total_token_counts(payload: &serde_json::Value) -> Option<(Option<u64>, Option<u64>)> {
    let pick = |v: &serde_json::Value, keys: &[&str]| -> Option<u64> {
        for key in keys {
            if let Some(num) = v.get(*key).and_then(|value| value.as_u64()) {
                return Some(num);
            }
            if let Some(num) = v
                .get(*key)
                .and_then(|value| value.as_i64())
                .filter(|value| *value >= 0)
                .map(|value| value as u64)
            {
                return Some(num);
            }
        }
        None
    };

    let total_usage = payload.get("info").and_then(|info| {
        info.get("total_token_usage")
            .or_else(|| info.get("totalTokenUsage"))
    })?;

    let input = pick(
        total_usage,
        &[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
            "token_input",
            "tokenInput",
        ],
    );
    let output = pick(
        total_usage,
        &[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
            "token_output",
            "tokenOutput",
        ],
    );

    if input.is_none() && output.is_none() {
        None
    } else {
        Some((input, output))
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

fn extract_message_text_blocks(content: Option<&serde_json::Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.trim().to_string();
    }
    let Some(blocks) = content.as_array() else {
        return String::new();
    };

    blocks
        .iter()
        .filter_map(|block| {
            if let Some(text) = block.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            let btype = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match btype {
                "text" | "input_text" | "output_text" => block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(String::from),
                _ => block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(String::from),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn looks_like_injected_codex_user_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();

    if lower.contains("apply_patch was requested via exec_command")
        && lower.contains("use the apply_patch tool instead")
    {
        return true;
    }

    lower == "agents.md instructions"
        || lower.starts_with("# agents.md instructions")
        || lower.contains("<instructions>")
        || lower.contains("</instructions>")
        || lower.contains("<environment_context>")
        || lower.contains("</environment_context>")
        || lower.contains("<turn_aborted>")
        || lower.contains("</turn_aborted>")
}

fn json_object_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(s) = map.get(*key).and_then(|entry| entry.as_str()) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = json_object_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                if let Some(found) = json_object_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
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

fn load_codex_agent_identity() -> (String, String) {
    let model = read_codex_model_from_config().unwrap_or_else(|| "unknown".to_string());
    let provider = read_codex_provider_from_config()
        .or_else(|| infer_provider_from_model(&model))
        .unwrap_or_else(|| "openai".to_string());
    (provider, model)
}

fn codex_config_path() -> Option<PathBuf> {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let home = codex_home.trim();
        if !home.is_empty() {
            return Some(PathBuf::from(home).join("config.toml"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    let home = home.trim();
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn read_codex_model_from_config() -> Option<String> {
    read_codex_setting_from_config("model")
}

fn read_codex_provider_from_config() -> Option<String> {
    read_codex_setting_from_config("provider")
        .or_else(|| read_codex_setting_from_config("model_provider"))
        .and_then(|provider| {
            let normalized = provider.trim().to_ascii_lowercase();
            if normalized.is_empty() || normalized == "auto" {
                None
            } else {
                Some(normalized)
            }
        })
}

fn read_codex_setting_from_config(key: &str) -> Option<String> {
    let path = codex_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    parse_codex_config_value(&text, key)
}

fn parse_codex_config_value(config_toml: &str, key: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(config_toml).ok()?;
    let active_profile = value
        .get("profile")
        .or_else(|| value.get("default_profile"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(profile) = active_profile {
        if let Some(profile_value) = value
            .get("profiles")
            .and_then(|profiles| profiles.get(profile))
            .and_then(|entry| entry.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(profile_value.to_string());
        }
    }
    if let Some(defaults_value) = value
        .get("defaults")
        .and_then(|defaults| defaults.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(defaults_value.to_string());
    }
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn infer_provider_from_model(model: &str) -> Option<String> {
    let lower = model.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "unknown" {
        return None;
    }
    if lower.contains("claude") {
        return Some("anthropic".to_string());
    }
    if lower.contains("gemini") {
        return Some("google".to_string());
    }
    if lower.contains("gpt")
        || lower.contains("openai")
        || lower.contains("codex")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
    {
        return Some("openai".to_string());
    }
    None
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

fn normalize_codex_function_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn extract_patch_target_path(args: &serde_json::Value) -> Option<String> {
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file"))
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(path.to_string());
    }

    for key in ["input", "patch"] {
        if let Some(path) = args
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(extract_patch_target_path_from_text)
        {
            return Some(path);
        }
    }

    None
}

fn extract_patch_target_path_from_text(input: &str) -> Option<String> {
    const PREFIXES: [&str; 3] = ["*** Update File:", "*** Add File:", "*** Delete File:"];
    for line in input.lines() {
        let trimmed = line.trim();
        for prefix in PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let path = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

fn classify_codex_function(name: &str, args: &serde_json::Value) -> EventType {
    let normalized_name = normalize_codex_function_name(name);
    match normalized_name {
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
            let path = extract_patch_target_path(args).unwrap_or_else(|| "unknown".to_string());
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
            name: normalized_name.to_string(),
        },
    }
}

fn codex_function_content(name: &str, args: &serde_json::Value) -> Content {
    match normalize_codex_function_name(name) {
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
    fn test_parse_codex_config_value_model_root() {
        let config = r#"
model = "gpt-5.3-codex"
model_reasoning_effort = "high"
"#;
        assert_eq!(
            parse_codex_config_value(config, "model"),
            Some("gpt-5.3-codex".to_string())
        );
    }

    #[test]
    fn test_parse_codex_config_value_profile_override() {
        let config = r#"
profile = "work"
model = "gpt-5.3-codex"
[profiles.work]
model = "claude-sonnet-4-5"
provider = "anthropic"
"#;
        assert_eq!(
            parse_codex_config_value(config, "model"),
            Some("claude-sonnet-4-5".to_string())
        );
        assert_eq!(
            parse_codex_config_value(config, "provider"),
            Some("anthropic".to_string())
        );
    }

    #[test]
    fn test_infer_provider_from_model() {
        assert_eq!(
            infer_provider_from_model("gpt-5.3-codex"),
            Some("openai".to_string())
        );
        assert_eq!(
            infer_provider_from_model("claude-sonnet-4-5"),
            Some("anthropic".to_string())
        );
        assert_eq!(
            infer_provider_from_model("gemini-2.0-flash"),
            Some("google".to_string())
        );
        assert_eq!(infer_provider_from_model("unknown"), None);
    }

    #[test]
    fn test_json_object_string_extracts_nested_branch_and_repo() {
        let git = serde_json::json!({
            "meta": {"repository": "ops"},
            "current": {"branch": "main"}
        });
        assert_eq!(
            json_object_string(&git, &["branch", "current_branch", "ref"]).as_deref(),
            Some("main")
        );
        assert_eq!(
            json_object_string(&git, &["repo_name", "repository", "repo"]).as_deref(),
            Some("ops")
        );
    }

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
    fn test_function_call_includes_semantic_metadata() {
        let call_line = r#"{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"call_meta_1"}"#;
        let output_line = r#"{"type":"function_call_output","call_id":"call_meta_1","output":"{\"output\":\"ok\",\"metadata\":{\"exit_code\":0,\"duration_seconds\":0.01}}"}"#;
        let mut events = Vec::new();
        let mut counter = 0u64;
        let mut first_text = None;
        let mut last_fn = "unknown".to_string();
        let mut call_map = HashMap::new();
        let ts = Utc::now();

        let call_value: serde_json::Value = serde_json::from_str(call_line).unwrap();
        process_item(
            &call_value,
            ts,
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );
        let output_value: serde_json::Value = serde_json::from_str(output_line).unwrap();
        process_item(
            &output_value,
            ts,
            &mut events,
            &mut counter,
            &mut first_text,
            &mut last_fn,
            &mut call_map,
        );

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]
                .attributes
                .get("semantic.call_id")
                .and_then(|v| v.as_str()),
            Some("call_meta_1")
        );
        assert_eq!(
            events[0]
                .attributes
                .get("semantic.tool_kind")
                .and_then(|v| v.as_str()),
            Some("shell")
        );
        assert_eq!(
            events[1]
                .attributes
                .get("semantic.call_id")
                .and_then(|v| v.as_str()),
            Some("call_meta_1")
        );
    }

    #[test]
    fn test_classify_update_plan() {
        let args = serde_json::json!({"plan": [{"step": "analyze", "status": "in_progress"}]});
        let et = classify_codex_function("update_plan", &args);
        assert!(matches!(et, EventType::ToolCall { name } if name == "update_plan"));
    }

    #[test]
    fn test_classify_apply_patch_uses_path_from_patch_input() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: crates/tui/src/ui.rs\n@@\n- old\n+ new\n*** End Patch\n"
        });
        let et = classify_codex_function("functions.apply_patch", &args);
        assert!(matches!(
            et,
            EventType::FileEdit { path, diff: None } if path == "crates/tui/src/ui.rs"
        ));
    }

    #[test]
    fn test_desktop_format_response_item() {
        // Desktop wraps entries in response_item with payload
        let lines = [
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
        // developer and injected user instruction messages are skipped.
        // Events: reasoning + shell_command + tool_result + assistant (+optional user)
        assert!(session.events.len() >= 4);
        assert!(!session.events.iter().any(|e| {
            matches!(e.event_type, EventType::UserMessage)
                && e.content.blocks.iter().any(|b| {
                    matches!(b, ContentBlock::Text { text } if text.contains("AGENTS.md instructions"))
                })
        }));
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

    #[test]
    fn test_desktop_warning_prompt_not_parsed_as_user_message() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:00:00.097Z","type":"session_meta","payload":{"id":"desktop-test-2","timestamp":"2026-02-14T10:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.119Z","type":"event_msg","payload":{"type":"user_message","message":"Warning: apply_patch was requested via exec_command. Use the apply_patch tool instead of exec_command."}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.120Z","type":"event_msg","payload":{"type":"user_message","message":"actual task please continue"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_warning_filter_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert_eq!(
            session.context.title.as_deref(),
            Some("actual task please continue")
        );
        assert!(!session.events.iter().any(|e| {
            matches!(e.event_type, EventType::UserMessage)
                && e.content.blocks.iter().any(|b| {
                    matches!(b, ContentBlock::Text { text } if text.contains("apply_patch was requested via exec_command"))
                })
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_agent_reasoning_event_msg_maps_to_thinking() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:00:00.097Z","type":"session_meta","payload":{"id":"desktop-reasoning","timestamp":"2026-02-14T13:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:00:01.000Z","type":"event_msg","payload":{"type":"agent_reasoning","message":"analyzing dependencies"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_agent_reasoning_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let reasoning_event = session.events.iter().find(|event| {
            matches!(event.event_type, EventType::Thinking)
                && event
                    .attributes
                    .get("source.raw_type")
                    .and_then(|v| v.as_str())
                    == Some("event_msg:agent_reasoning")
        });
        assert!(reasoning_event.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_token_count_event_msg_maps_to_custom_tokens() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:10:00.097Z","type":"session_meta","payload":{"id":"desktop-token-count","timestamp":"2026-02-14T13:10:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:10:01.000Z","type":"event_msg","payload":{"type":"token_count","input_tokens":21,"output_tokens":8,"turn_id":"turn-xyz"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_token_count_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let token_event = session.events.iter().find(|event| {
            matches!(
                event.event_type,
                EventType::Custom { ref kind } if kind == "token_count"
            )
        });
        assert!(token_event.is_some());
        let token_event = token_event.unwrap();
        assert_eq!(
            token_event
                .attributes
                .get("input_tokens")
                .and_then(|v| v.as_u64()),
            Some(21)
        );
        assert_eq!(
            token_event
                .attributes
                .get("output_tokens")
                .and_then(|v| v.as_u64()),
            Some(8)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_token_count_event_msg_info_usage_maps_to_custom_tokens() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:11:00.097Z","type":"session_meta","payload":{"id":"desktop-token-count-info","timestamp":"2026-02-14T13:11:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:11:01.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":34,"output_tokens":13}},"turn_id":"turn-info"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_token_count_info_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let token_event = session.events.iter().find(|event| {
            matches!(
                event.event_type,
                EventType::Custom { ref kind } if kind == "token_count"
            )
        });
        assert!(token_event.is_some());
        let token_event = token_event.unwrap();
        assert_eq!(
            token_event
                .attributes
                .get("input_tokens")
                .and_then(|v| v.as_u64()),
            Some(34)
        );
        assert_eq!(
            token_event
                .attributes
                .get("output_tokens")
                .and_then(|v| v.as_u64()),
            Some(13)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_token_count_event_msg_includes_cumulative_totals() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:13:00.097Z","type":"session_meta","payload":{"id":"desktop-token-count-total","timestamp":"2026-02-14T13:13:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:13:01.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":34,"output_tokens":13},"total_token_usage":{"input_tokens":340,"output_tokens":130}},"turn_id":"turn-total"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_token_count_total_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let token_event = session.events.iter().find(|event| {
            matches!(
                event.event_type,
                EventType::Custom { ref kind } if kind == "token_count"
            )
        });
        assert!(token_event.is_some());
        let token_event = token_event.unwrap();
        assert_eq!(
            token_event
                .attributes
                .get("input_tokens")
                .and_then(|v| v.as_u64()),
            Some(34)
        );
        assert_eq!(
            token_event
                .attributes
                .get("output_tokens")
                .and_then(|v| v.as_u64()),
            Some(13)
        );
        assert_eq!(
            token_event
                .attributes
                .get("input_tokens_total")
                .and_then(|v| v.as_u64()),
            Some(340)
        );
        assert_eq!(
            token_event
                .attributes
                .get("output_tokens_total")
                .and_then(|v| v.as_u64()),
            Some(130)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_agent_reasoning_raw_content_maps_to_thinking() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:12:00.097Z","type":"session_meta","payload":{"id":"desktop-raw-reasoning","timestamp":"2026-02-14T13:12:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:12:01.000Z","type":"event_msg","payload":{"type":"agent_reasoning_raw_content","text":"hidden chain tokenized text"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_reasoning_raw_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let reasoning_event = session.events.iter().find(|event| {
            matches!(event.event_type, EventType::Thinking)
                && event
                    .attributes
                    .get("source.raw_type")
                    .and_then(|v| v.as_str())
                    == Some("event_msg:agent_reasoning_raw_content")
        });
        assert!(reasoning_event.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_web_search_call_actions_map_to_web_events() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:20:00.097Z","type":"session_meta","payload":{"id":"desktop-web-search","timestamp":"2026-02-14T13:20:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:20:01.000Z","type":"response_item","payload":{"type":"web_search_call","status":"completed","id":"ws_1","action":{"type":"search","query":"weather seattle","queries":["weather seattle","seattle forecast"]}}}"#,
            r#"{"timestamp":"2026-02-14T13:20:01.500Z","type":"response_item","payload":{"type":"web_search_call","status":"completed","action":{"type":"open_page","url":"https://example.com/weather"}}}"#,
            r#"{"timestamp":"2026-02-14T13:20:02.000Z","type":"response_item","payload":{"type":"web_search_call","status":"completed","action":{"type":"find_in_page","url":"https://example.com/weather","pattern":"rain"}}}"#,
            r#"{"timestamp":"2026-02-14T13:20:02.500Z","type":"response_item","payload":{"type":"web_search_call","status":"completed","action":{"type":"open_page"}}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_web_search_actions_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert!(session.events.iter().any(|event| {
            matches!(&event.event_type, EventType::WebSearch { query } if query == "weather seattle")
                && event
                    .attributes
                    .get("source.raw_type")
                    .and_then(|v| v.as_str())
                    == Some("web_search_call:search")
                && event
                    .attributes
                    .get("semantic.call_id")
                    .and_then(|v| v.as_str())
                    == Some("ws_1")
                && event
                    .attributes
                    .get("web_search.queries")
                    .and_then(|v| v.as_array())
                    .map(|queries| queries.len())
                    == Some(2)
        }));
        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::WebFetch { url } if url == "https://example.com/weather"
            ) && event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("web_search_call:open_page")
        }));
        assert!(session.events.iter().any(|event| {
            event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("web_search_call:find_in_page")
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("pattern: rain"))
                })
        }));
        assert!(session.events.iter().any(|event| {
            matches!(&event.event_type, EventType::ToolCall { name } if name == "web_search")
                && event
                    .attributes
                    .get("source.raw_type")
                    .and_then(|v| v.as_str())
                    == Some("web_search_call:open_page")
                && event.content.blocks.iter().any(
                    |block| matches!(block, ContentBlock::Text { text } if text == "open_page"),
                )
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_context_compacted_event_msg_maps_to_custom() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:30:00.097Z","type":"session_meta","payload":{"id":"desktop-context-compacted","timestamp":"2026-02-14T13:30:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:30:01.000Z","type":"event_msg","payload":{"type":"context_compacted","turn_id":"turn_cc_1"}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_context_compacted_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::Custom { kind } if kind == "context_compacted"
            ) && event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("event_msg:context_compacted")
                && event.task_id.as_deref() == Some("turn_cc_1")
        }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_item_completed_plan_maps_to_custom() {
        let lines = [
            r#"{"timestamp":"2026-02-14T13:31:00.097Z","type":"session_meta","payload":{"id":"desktop-item-completed","timestamp":"2026-02-14T13:31:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
            r#"{"timestamp":"2026-02-14T13:31:01.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"turn_plan_1","item":{"type":"Plan","id":"plan_1","text":"Investigate parser drift\n- check fixtures"}}}"#,
        ];
        let dir = std::env::temp_dir().join("codex_desktop_item_completed_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert!(session.events.iter().any(|event| {
            matches!(
                &event.event_type,
                EventType::Custom { kind } if kind == "plan_completed"
            ) && event
                .attributes
                .get("source.raw_type")
                .and_then(|v| v.as_str())
                == Some("event_msg:item_completed")
                && event
                    .attributes
                    .get("plan_id")
                    .and_then(|v| v.as_str())
                    == Some("plan_1")
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("Investigate parser drift"))
                })
        }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_turn_aborted_filtered_from_user_messages() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:00:00.097Z","type":"session_meta","payload":{"id":"desktop-test-3","timestamp":"2026-02-14T10:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.119Z","type":"event_msg","payload":{"type":"user_message","message":"<turn_aborted>Request interrupted by user for tool use</turn_aborted>"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.150Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn_1","message":"user interrupted"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.200Z","type":"event_msg","payload":{"type":"user_message","message":"real user prompt"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_turn_aborted_filter_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert_eq!(session.context.title.as_deref(), Some("real user prompt"));
        assert!(session.events.iter().any(|event| {
            matches!(
                event.event_type,
                EventType::Custom { ref kind } if kind == "turn_aborted"
            )
        }));
        assert!(!session.events.iter().any(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && event.content.blocks.iter().any(
                    |block| matches!(block, ContentBlock::Text { text } if text.contains("turn_aborted"))
                )
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_task_lifecycle_event_msg_maps_to_task_events() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:00:00.097Z","type":"session_meta","payload":{"id":"desktop-task-map","timestamp":"2026-02-14T10:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.120Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn_42","title":"Investigate bug"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.500Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"working"}]}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.900Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn_42","last_agent_message":"fixed and validated"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_task_map_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::TaskStart { .. })
                && event.task_id.as_deref() == Some("turn_42")
        }));
        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::TaskEnd { .. })
                && event.task_id.as_deref() == Some("turn_42")
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_task_complete_last_agent_message_promoted_to_agent_message() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:05:00.097Z","type":"session_meta","payload":{"id":"desktop-task-summary-promote","timestamp":"2026-02-14T10:05:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:05:00.120Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn_55","title":"Investigate bug"}}"#,
            r#"{"timestamp":"2026-02-14T10:05:00.900Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn_55","last_agent_message":"fixed and validated"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_task_summary_promote_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let agent_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::AgentMessage))
            .collect();
        assert_eq!(agent_events.len(), 1);
        assert_eq!(
            agent_events[0]
                .attributes
                .get("source")
                .and_then(|value| value.as_str()),
            Some("event_msg")
        );
        assert!(agent_events[0].content.blocks.iter().any(
            |block| matches!(block, ContentBlock::Text { text } if text.contains("fixed and validated"))
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_task_complete_last_agent_message_dedupes_with_agent_message() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:06:00.097Z","type":"session_meta","payload":{"id":"desktop-task-summary-dedupe","timestamp":"2026-02-14T10:06:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:06:00.300Z","type":"event_msg","payload":{"type":"agent_message","message":"fixed and validated"}}"#,
            r#"{"timestamp":"2026-02-14T10:06:00.900Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn_56","last_agent_message":"fixed and validated"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_task_summary_dedupe_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let agent_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::AgentMessage))
            .collect();
        assert_eq!(agent_events.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_unmatched_task_started_is_synthetically_closed() {
        let lines = [
            r#"{"timestamp":"2026-02-14T10:00:00.097Z","type":"session_meta","payload":{"id":"desktop-task-close","timestamp":"2026-02-14T10:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.120Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn_99","title":"Long task"}}"#,
            r#"{"timestamp":"2026-02-14T10:00:00.500Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"still running"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_task_close_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let maybe_end = session.events.iter().find(|event| {
            matches!(
                event.event_type,
                EventType::TaskEnd {
                    summary: Some(ref s)
                } if s.contains("synthetic end")
            ) && event.task_id.as_deref() == Some("turn_99")
        });
        assert!(maybe_end.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_event_msg_user_message_preferred_over_response_fallback() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:00:00.000Z","type":"session_meta","payload":{"id":"desktop-user-priority","timestamp":"2026-02-14T11:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:00:00.100Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"same user prompt"}]}}"#,
            r#"{"timestamp":"2026-02-14T11:00:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"same user prompt"}}"#,
            r#"{"timestamp":"2026-02-14T11:00:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_user_priority_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let user_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::UserMessage))
            .collect();
        assert_eq!(user_events.len(), 1);
        assert_eq!(
            user_events[0]
                .attributes
                .get("source")
                .and_then(|value| value.as_str()),
            Some("event_msg")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_event_msg_dedupes_response_fallback_with_image_marker() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:10:00.000Z","type":"session_meta","payload":{"id":"desktop-user-image-dedupe","timestamp":"2026-02-14T11:10:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:10:00.100Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"same user prompt\n<image>"}]}}"#,
            r#"{"timestamp":"2026-02-14T11:10:01.000Z","type":"event_msg","payload":{"type":"user_message","message":"same user prompt"}}"#,
            r#"{"timestamp":"2026-02-14T11:10:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_user_image_dedupe_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let user_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::UserMessage))
            .collect();
        assert_eq!(user_events.len(), 1);
        assert_eq!(
            user_events[0]
                .attributes
                .get("source")
                .and_then(|value| value.as_str()),
            Some("event_msg")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_event_msg_same_source_duplicates_are_collapsed() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:20:00.000Z","type":"session_meta","payload":{"id":"desktop-user-same-source-dedupe","timestamp":"2026-02-14T11:20:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:20:00.100Z","type":"event_msg","payload":{"type":"user_message","message":"same user prompt"}}"#,
            r#"{"timestamp":"2026-02-14T11:20:00.900Z","type":"event_msg","payload":{"type":"user_message","message":"same user prompt"}}"#,
            r#"{"timestamp":"2026-02-14T11:20:02.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_same_source_dedupe_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let user_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::UserMessage))
            .collect();
        assert_eq!(user_events.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_event_msg_agent_message_preferred_over_response_fallback() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:30:00.000Z","type":"session_meta","payload":{"id":"desktop-agent-priority","timestamp":"2026-02-14T11:30:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:30:00.100Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"same assistant reply"}]}}"#,
            r#"{"timestamp":"2026-02-14T11:30:01.000Z","type":"event_msg","payload":{"type":"agent_message","message":"same assistant reply"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_agent_priority_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let agent_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::AgentMessage))
            .collect();
        assert_eq!(agent_events.len(), 1);
        assert_eq!(
            agent_events[0]
                .attributes
                .get("source")
                .and_then(|value| value.as_str()),
            Some("event_msg")
        );
        assert!(agent_events[0].content.blocks.iter().any(
            |block| matches!(block, ContentBlock::Text { text } if text.contains("same assistant reply"))
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_event_msg_agent_message_same_source_duplicates_are_collapsed() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:40:00.000Z","type":"session_meta","payload":{"id":"desktop-agent-same-source-dedupe","timestamp":"2026-02-14T11:40:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:40:00.100Z","type":"event_msg","payload":{"type":"agent_message","message":"same assistant reply"}}"#,
            r#"{"timestamp":"2026-02-14T11:40:00.900Z","type":"event_msg","payload":{"type":"agent_message","message":"same assistant reply"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_agent_same_source_dedupe_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let agent_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::AgentMessage))
            .collect();
        assert_eq!(agent_events.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_desktop_response_fallback_agent_message_kept_without_event_msg() {
        let lines = [
            r#"{"timestamp":"2026-02-14T11:50:00.000Z","type":"session_meta","payload":{"id":"desktop-agent-response-fallback","timestamp":"2026-02-14T11:50:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T11:50:00.100Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"assistant only response"}]}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_agent_response_fallback_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        let agent_events: Vec<&Event> = session
            .events
            .iter()
            .filter(|event| matches!(event.event_type, EventType::AgentMessage))
            .collect();
        assert_eq!(agent_events.len(), 1);
        assert_eq!(
            agent_events[0]
                .attributes
                .get("source")
                .and_then(|value| value.as_str()),
            Some("response_fallback")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_request_user_input_output_promoted_to_interactive_user_message() {
        let lines = [
            r#"{"timestamp":"2026-02-14T12:00:00.000Z","type":"session_meta","payload":{"id":"desktop-request-user-input","timestamp":"2026-02-14T12:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
            r#"{"timestamp":"2026-02-14T12:00:00.100Z","type":"response_item","payload":{"type":"function_call","name":"request_user_input","arguments":"{\"questions\":[{\"id\":\"layout_mode\",\"header\":\"Layout\",\"question\":\"Select mode\"}] }","call_id":"call_req_1"}}"#,
            r#"{"timestamp":"2026-02-14T12:00:01.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_req_1","output":"{\"answers\":{\"layout_mode\":{\"answers\":[\"Always multi-column\"]}}}"}}"#,
        ];

        let dir = std::env::temp_dir().join("codex_desktop_request_user_input_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rollout-test.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();

        let session = parse_codex_jsonl(&path).unwrap();
        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && event
                    .attributes
                    .get("source")
                    .and_then(|value| value.as_str())
                    == Some("interactive")
                && event
                    .attributes
                    .get("call_id")
                    .and_then(|value| value.as_str())
                    == Some("call_req_1")
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("layout_mode: Always multi-column") && !text.contains("Interactive response"))
                })
        }));
        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::SystemMessage)
                && event
                    .attributes
                    .get("source")
                    .and_then(|value| value.as_str())
                    == Some("interactive_question")
                && event
                    .attributes
                    .get("question_meta")
                    .and_then(|value| value.as_array())
                    .is_some()
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("Select mode"))
                })
        }));
        assert!(session.events.iter().any(|event| {
            matches!(event.event_type, EventType::ToolResult { ref name, .. } if name == "request_user_input")
        }));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
