use super::*;

pub(crate) fn parse_codex_jsonl(path: &Path) -> Result<Session> {
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
    let mut parent_session_id: Option<String> = None;
    let mut is_auxiliary_session = false;
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
                if codex_desktop_payload_is_auxiliary(payload) {
                    is_auxiliary_session = true;
                }
                set_first(
                    &mut parent_session_id,
                    codex_desktop_parent_session_id(payload),
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
                if payload.get("type").and_then(|v| v.as_str()) == Some("message")
                    && payload.get("role").and_then(|v| v.as_str()) == Some("user")
                    && looks_like_summary_batch_prompt(&extract_message_text_blocks(
                        payload.get("content"),
                    ))
                {
                    is_auxiliary_session = true;
                }
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
                            if looks_like_summary_batch_prompt(&text) {
                                is_auxiliary_session = true;
                            }
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
    if first_user_text
        .as_deref()
        .is_some_and(looks_like_summary_batch_prompt)
    {
        is_auxiliary_session = true;
    }
    let mut related_session_ids = Vec::new();
    if is_auxiliary_session {
        attributes.insert(
            ATTR_SESSION_ROLE.to_string(),
            serde_json::Value::String("auxiliary".to_string()),
        );
        if let Some(parent_id) = parent_session_id.as_ref() {
            attributes.insert(
                ATTR_PARENT_SESSION_ID.to_string(),
                serde_json::Value::String(parent_id.clone()),
            );
            related_session_ids.push(parent_id.clone());
        }
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
        related_session_ids,
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}
