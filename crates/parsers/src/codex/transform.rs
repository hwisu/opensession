use super::*;

#[cfg(test)]
pub(super) fn process_item(
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
pub(super) fn process_item_with_options(
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
            let raw_name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let name = canonical_tool_name(raw_name);
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
            if name == INTERACTIVE_USER_INPUT_TOOL {
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

            if call_name == INTERACTIVE_USER_INPUT_TOOL {
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

pub(super) fn push_user_message_event(
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

pub(super) fn push_agent_message_event(
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
