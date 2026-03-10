use super::*;

#[test]
fn test_desktop_agent_reasoning_raw_content_maps_to_thinking() {
    let lines = [
        r#"{"timestamp":"2026-02-14T13:12:00.097Z","type":"session_meta","payload":{"id":"desktop-raw-reasoning","timestamp":"2026-02-14T13:12:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
        r#"{"timestamp":"2026-02-14T13:12:01.000Z","type":"event_msg","payload":{"type":"agent_reasoning_raw_content","text":"hidden chain tokenized text"}}"#,
    ];
    let dir = temp_test_dir("codex_desktop_reasoning_raw_test");
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
    let dir = temp_test_dir("codex_desktop_web_search_actions_test");
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
            && event
                .content
                .blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::Text { text } if text == "open_page"))
    }));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_desktop_context_compacted_event_msg_maps_to_custom() {
    let lines = [
        r#"{"timestamp":"2026-02-14T13:30:00.097Z","type":"session_meta","payload":{"id":"desktop-context-compacted","timestamp":"2026-02-14T13:30:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.101.0"}}"#,
        r#"{"timestamp":"2026-02-14T13:30:01.000Z","type":"event_msg","payload":{"type":"context_compacted","turn_id":"turn_cc_1"}}"#,
    ];
    let dir = temp_test_dir("codex_desktop_context_compacted_test");
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
    let dir = temp_test_dir("codex_desktop_item_completed_test");
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

    let dir = temp_test_dir("codex_desktop_turn_aborted_filter_test");
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
fn test_desktop_subagent_notification_filtered_from_user_messages() {
    let lines = [
        r#"{"timestamp":"2026-03-03T06:39:35.000Z","type":"session_meta","payload":{"id":"desktop-subagent-notification","timestamp":"2026-03-03T06:39:35.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.108.0"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:35.100Z","type":"event_msg","payload":{"type":"user_message","message":"<subagent_notification>Event: 99/110</subagent_notification>"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:35.200Z","type":"event_msg","payload":{"type":"user_message","message":"real user prompt"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:36.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}"#,
    ];

    let dir = temp_test_dir("codex_desktop_subagent_notification_filter_test");
    let path = dir.join("rollout-test.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();

    let session = parse_codex_jsonl(&path).unwrap();
    assert_eq!(session.context.title.as_deref(), Some("real user prompt"));
    assert!(!session.events.iter().any(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("subagent_notification"))
                })
        }));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_desktop_subagent_notification_prefix_line_filtered_from_user_messages() {
    let lines = [
        r#"{"timestamp":"2026-03-03T06:39:35.000Z","type":"session_meta","payload":{"id":"desktop-subagent-notification-2","timestamp":"2026-03-03T06:39:35.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.108.0"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:35.100Z","type":"event_msg","payload":{"type":"user_message","message":"[USER] <subagent_notification>\nEvent: 99/110"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:35.200Z","type":"event_msg","payload":{"type":"user_message","message":"real user prompt"}}"#,
        r#"{"timestamp":"2026-03-03T06:39:36.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}"#,
    ];

    let dir = temp_test_dir("codex_desktop_subagent_notification_filter_test_2");
    let path = dir.join("rollout-test.jsonl");
    std::fs::write(&path, lines.join("\n")).unwrap();

    let session = parse_codex_jsonl(&path).unwrap();
    assert_eq!(session.context.title.as_deref(), Some("real user prompt"));
    assert!(!session.events.iter().any(|event| {
            matches!(event.event_type, EventType::UserMessage)
                && event.content.blocks.iter().any(|block| {
                    matches!(block, ContentBlock::Text { text } if text.contains("subagent_notification"))
                })
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

    let dir = temp_test_dir("codex_desktop_task_map_test");
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

    let dir = temp_test_dir("codex_desktop_task_summary_promote_test");
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

    let dir = temp_test_dir("codex_desktop_task_summary_dedupe_test");
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

    let dir = temp_test_dir("codex_desktop_task_close_test");
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

    let dir = temp_test_dir("codex_desktop_user_priority_test");
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

    let dir = temp_test_dir("codex_desktop_user_image_dedupe_test");
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

    let dir = temp_test_dir("codex_desktop_same_source_dedupe_test");
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

    let dir = temp_test_dir("codex_desktop_agent_priority_test");
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

    let dir = temp_test_dir("codex_desktop_agent_same_source_dedupe_test");
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

    let dir = temp_test_dir("codex_desktop_agent_response_fallback_test");
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

    let dir = temp_test_dir("codex_desktop_request_user_input_test");
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
