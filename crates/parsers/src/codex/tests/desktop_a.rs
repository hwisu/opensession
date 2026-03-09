use super::*;

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

    let dir = temp_test_dir("codex_desktop_test");
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

    let dir = temp_test_dir("codex_desktop_warning_filter_test");
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
fn test_desktop_subagent_thread_spawn_marks_session_auxiliary() {
    let lines = [
        r#"{"timestamp":"2026-02-27T04:49:06.449Z","type":"session_meta","payload":{"id":"desktop-subagent","timestamp":"2026-02-27T04:49:05.467Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.105.0","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-thread-1","depth":1,"agent_role":"awaiter"}}},"agent_role":"awaiter"}}"#,
        r#"{"timestamp":"2026-02-27T04:49:06.451Z","type":"event_msg","payload":{"type":"agent_message","message":"child response"}}"#,
    ];

    let dir = temp_test_dir("codex_desktop_subagent_auxiliary_test");
    let path = dir.join("rollout-test.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();

    let session = parse_codex_jsonl(&path).unwrap();
    assert_eq!(
        session
            .context
            .attributes
            .get(ATTR_SESSION_ROLE)
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert_eq!(
        session
            .context
            .attributes
            .get(ATTR_PARENT_SESSION_ID)
            .and_then(|value| value.as_str()),
        Some("parent-thread-1")
    );
    assert_eq!(
        session.context.related_session_ids,
        vec!["parent-thread-1".to_string()]
    );
    assert!(opensession_core::session::is_auxiliary_session(&session));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_desktop_agent_role_awaiter_marks_session_auxiliary() {
    let lines = [
        r#"{"timestamp":"2026-02-27T04:49:06.449Z","type":"session_meta","payload":{"id":"desktop-agent-role-subagent","timestamp":"2026-02-27T04:49:05.467Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.105.0","agent_role":"awaiter"}}"#,
        r#"{"timestamp":"2026-02-27T04:49:06.451Z","type":"event_msg","payload":{"type":"agent_message","message":"child response"}}"#,
    ];

    let dir = temp_test_dir("codex_desktop_agent_role_auxiliary_test");
    let path = dir.join("rollout-test.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();

    let session = parse_codex_jsonl(&path).unwrap();
    assert_eq!(
        session
            .context
            .attributes
            .get(ATTR_SESSION_ROLE)
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert!(opensession_core::session::is_auxiliary_session(&session));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_desktop_summary_batch_prompt_marks_session_auxiliary() {
    let lines = [
        r#"{"timestamp":"2026-03-05T02:06:54.719Z","type":"session_meta","payload":{"id":"desktop-summary-worker","timestamp":"2026-03-05T02:06:54.649Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.104.0","source":"exec"}}"#,
        r#"{"timestamp":"2026-03-05T02:06:54.721Z","type":"event_msg","payload":{"type":"user_message","message":"Convert a real coding session into semantic compression.\nPipeline: session -> HAIL compact -> semantic summary.\nHAIL_COMPACT={\"session\":{\"id\":\"s1\"}}"}}"#,
        r#"{"timestamp":"2026-03-05T02:07:01.792Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"{\"changes\":\"none\"}"}],"phase":"final_answer"}}"#,
    ];

    let dir = temp_test_dir("codex_desktop_summary_worker_auxiliary_test");
    let path = dir.join("rollout-test.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();

    let session = parse_codex_jsonl(&path).unwrap();
    assert_eq!(
        session
            .context
            .attributes
            .get(ATTR_SESSION_ROLE)
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert!(
        session.context.title.is_none(),
        "summary worker prompt should be excluded from visible title"
    );
    assert!(opensession_core::session::is_auxiliary_session(&session));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_desktop_agent_reasoning_event_msg_maps_to_thinking() {
    let lines = [
        r#"{"timestamp":"2026-02-14T13:00:00.097Z","type":"session_meta","payload":{"id":"desktop-reasoning","timestamp":"2026-02-14T13:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
        r#"{"timestamp":"2026-02-14T13:00:01.000Z","type":"event_msg","payload":{"type":"agent_reasoning","message":"analyzing dependencies"}}"#,
    ];
    let dir = temp_test_dir("codex_desktop_agent_reasoning_test");
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
    let dir = temp_test_dir("codex_desktop_token_count_test");
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
    let dir = temp_test_dir("codex_desktop_token_count_info_test");
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
    let dir = temp_test_dir("codex_desktop_token_count_total_test");
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
