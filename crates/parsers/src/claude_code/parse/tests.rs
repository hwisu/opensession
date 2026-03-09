use super::*;
use chrono::Datelike;
use chrono::Duration;
use std::collections::HashMap;
use std::fs::{create_dir_all, write};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_temp_root() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("opensession-claude-parser-{nanos}"));
    create_dir_all(&path).expect("create test temp root");
    path
}

#[test]
fn test_parse_timestamp() {
    let ts = parse_timestamp("2026-02-06T04:46:17.839Z").unwrap();
    assert_eq!(ts.year(), 2026);
}

#[test]
fn test_raw_entry_deserialization_user_string() {
    let json = r#"{"type":"user","uuid":"abc","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"hello"}}"#;
    let entry: RawEntry = serde_json::from_str(json).unwrap();
    match entry {
        RawEntry::User(conv) => {
            assert_eq!(conv.uuid, "abc");
            match conv.message.content {
                RawContent::Text(text) => assert_eq!(text, "hello"),
                _ => panic!("Expected text content"),
            }
        }
        _ => panic!("Expected User entry"),
    }
}

#[test]
fn test_raw_entry_deserialization_assistant() {
    let json = r#"{"type":"assistant","uuid":"def","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"assistant","model":"claude-opus-4-6","content":[{"type":"text","text":"hi"}]}}"#;
    let entry: RawEntry = serde_json::from_str(json).unwrap();
    match entry {
        RawEntry::Assistant(conv) => {
            assert_eq!(conv.message.model.as_deref(), Some("claude-opus-4-6"));
        }
        _ => panic!("Expected Assistant entry"),
    }
}

#[test]
fn test_raw_entry_skip_file_history() {
    let json = r#"{"type":"file-history-snapshot","messageId":"abc","snapshot":{},"isSnapshotUpdate":false}"#;
    let entry: RawEntry = serde_json::from_str(json).unwrap();
    matches!(entry, RawEntry::FileHistorySnapshot { .. });
}

#[test]
fn test_raw_entry_deserialization_queue_operation_and_summary() {
    let queue_json = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-01-01T00:00:01Z","sessionId":"s1","content":"queued"}"#;
    let queue_entry: RawEntry = serde_json::from_str(queue_json).unwrap();
    match queue_entry {
        RawEntry::QueueOperation(entry) => {
            assert_eq!(entry.operation.as_deref(), Some("enqueue"));
            assert_eq!(entry.content.as_deref(), Some("queued"));
            assert_eq!(entry.session_id.as_deref(), Some("s1"));
        }
        _ => panic!("Expected QueueOperation entry"),
    }

    let summary_json = r#"{"type":"summary","summary":"Fix parser edge case","leafUuid":"leaf-1"}"#;
    let summary_entry: RawEntry = serde_json::from_str(summary_json).unwrap();
    match summary_entry {
        RawEntry::Summary(entry) => {
            assert_eq!(entry.summary.as_deref(), Some("Fix parser edge case"));
            assert_eq!(entry.leaf_uuid.as_deref(), Some("leaf-1"));
        }
        _ => panic!("Expected Summary entry"),
    }
}

#[test]
fn test_parse_lines_includes_system_progress_queue_and_summary_events() {
    let lines = vec![
        serde_json::json!({
            "type": "system",
            "uuid": "sys-1",
            "sessionId": "s1",
            "timestamp": "2026-01-01T00:00:00Z",
            "gitBranch": "feature/session-branch",
            "subtype": "local_command",
            "content": "<command-name>/usage</command-name>"
        })
        .to_string(),
        serde_json::json!({
            "type": "progress",
            "uuid": "prog-1",
            "sessionId": "s1",
            "timestamp": "2026-01-01T00:00:01Z",
            "toolUseID": "tool-123",
            "data": {
                "type": "hook_progress",
                "hookEvent": "PreToolUse",
                "hookName": "PreToolUse:Task"
            }
        })
        .to_string(),
        serde_json::json!({
            "type": "queue-operation",
            "sessionId": "s1",
            "timestamp": "2026-01-01T00:00:02Z",
            "operation": "enqueue",
            "content": "queued input"
        })
        .to_string(),
        serde_json::json!({
            "type": "summary",
            "sessionId": "s1",
            "leafUuid": "leaf-1",
            "summary": "Fix parser edge case"
        })
        .to_string(),
    ];

    let parsed = parse_lines_impl(&lines);
    assert_eq!(parsed.events.len(), 4);
    assert_eq!(parsed.session_id.as_deref(), Some("s1"));
    assert!(
        parsed
            .events
            .iter()
            .all(|event| matches!(event.event_type, EventType::SystemMessage))
    );

    let mut seen_raw_types = HashMap::new();
    for event in &parsed.events {
        let raw_type = event
            .attributes
            .get("source.raw_type")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        seen_raw_types.insert(raw_type, event.event_id.clone());
    }

    assert!(seen_raw_types.contains_key("system"));
    assert!(seen_raw_types.contains_key("progress"));
    assert!(seen_raw_types.contains_key("queue-operation"));
    assert!(seen_raw_types.contains_key("summary"));
    let context = parsed.context.expect("context from parsed lines");
    assert_eq!(
        context
            .attributes
            .get("git_branch")
            .and_then(|value| value.as_str()),
        Some("feature/session-branch")
    );
}

#[test]
fn test_tool_result_without_tool_use_id_falls_back_to_recent_tool_use() {
    let assistant_json = r#"{
        "type":"assistant",
        "uuid":"a1",
        "sessionId":"s1",
        "timestamp":"2026-02-01T00:00:00Z",
        "message":{
            "role":"assistant",
            "model":"claude-opus-4-6",
            "content":[
                {"type":"tool_use","name":"Read","input":{"file_path":"src/main.rs"}}
            ]
        }
    }"#;
    let user_json = r#"{
        "type":"user",
        "uuid":"u1",
        "sessionId":"s1",
        "timestamp":"2026-02-01T00:00:01Z",
        "message":{
            "role":"user",
            "content":[
                {"type":"tool_result","content":"ok","is_error":false}
            ]
        }
    }"#;

    let assistant_entry: RawEntry = serde_json::from_str(assistant_json).unwrap();
    let user_entry: RawEntry = serde_json::from_str(user_json).unwrap();
    let mut events = Vec::new();
    let mut tool_use_info = HashMap::new();

    match assistant_entry {
        RawEntry::Assistant(conv) => {
            process_assistant_entry(
                &conv,
                parse_timestamp(&conv.timestamp).unwrap(),
                &mut events,
                &mut tool_use_info,
            );
        }
        _ => panic!("expected assistant entry"),
    }
    match user_entry {
        RawEntry::User(conv) => {
            process_user_entry(
                &conv,
                parse_timestamp(&conv.timestamp).unwrap(),
                &mut events,
                &tool_use_info,
            );
        }
        _ => panic!("expected user entry"),
    }

    let result_event = events
        .iter()
        .find(|event| matches!(event.event_type, EventType::ToolResult { .. }))
        .expect("tool result exists");
    match &result_event.event_type {
        EventType::ToolResult { name, .. } => assert_eq!(name, "Read"),
        _ => unreachable!(),
    }
}

#[test]
fn test_subagent_file_merge_handles_file_name_without_meta() {
    let dir = test_temp_root();
    let parent_path = dir.as_path().join("session-parent.jsonl");
    let subagent_dir = parent_path.with_extension("").join("subagents");
    create_dir_all(&subagent_dir).unwrap();

    let parent_session = "sess-parent";
    let subagent_session = "agent-abc123";

    let parent_entry = serde_json::json!({
        "type": "user",
        "uuid": "u1",
        "sessionId": parent_session,
        "timestamp": Utc::now().to_rfc3339(),
        "message": {
            "role": "user",
            "content": "parent prompt"
        }
    })
    .to_string();
    write(&parent_path, parent_entry).unwrap();

    let subagent_entry = serde_json::json!({
        "type": "assistant",
        "uuid": "a1",
        "sessionId": subagent_session,
        "timestamp": Utc::now()
            .checked_add_signed(Duration::seconds(1))
            .unwrap()
            .to_rfc3339(),
        "message": {
            "role": "assistant",
            "model": "claude-3-opus",
            "content": [{
                "type": "text",
                "text": "subagent reply"
            }]
        }
    })
    .to_string();
    write(
        subagent_dir.join(format!("{subagent_session}.jsonl")),
        subagent_entry,
    )
    .unwrap();

    let session = parse_claude_code_jsonl(&parent_path).unwrap();
    assert_eq!(session.events.len(), 4);
    assert!(
        session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::TaskStart { .. }))
    );
    assert!(session.events.iter().any(|event| {
        event
            .attributes
            .get("merged_subagent")
            .and_then(|value| value.as_bool())
            == Some(true)
    }));
    assert!(
        session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::AgentMessage))
    );
    assert!(
        session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::TaskEnd { .. }))
    );
    assert_eq!(session.stats.message_count, 3);
}

#[test]
fn test_subagent_file_merge_handles_sibling_layout_with_parent_id_meta() {
    let dir = test_temp_root();
    let parent_path = dir.as_path().join("session-parent-sibling.jsonl");
    let parent_session = "sess-parent-sibling";

    let parent_entry = serde_json::json!({
        "type": "user",
        "uuid": "u1",
        "sessionId": parent_session,
        "timestamp": Utc::now().to_rfc3339(),
        "message": {
            "role": "user",
            "content": "parent prompt"
        }
    })
    .to_string();
    write(&parent_path, parent_entry).unwrap();

    let sibling_subagent_path = dir
        .as_path()
        .join("70dafb43-dbdd-4009-beb0-b6ac2bd9c4d1.jsonl");
    let subagent_entry = serde_json::json!({
        "type": "assistant",
        "uuid": "a1",
        "sessionId": "subagent-random",
        "parentUuid": parent_session,
        "timestamp": Utc::now()
            .checked_add_signed(Duration::seconds(1))
            .unwrap()
            .to_rfc3339(),
        "message": {
            "role": "assistant",
            "model": "claude-3-opus",
            "content": [{
                "type": "text",
                "text": "sibling subagent reply"
            }]
        }
    })
    .to_string();
    write(&sibling_subagent_path, subagent_entry).unwrap();

    let session = parse_claude_code_jsonl(&parent_path).unwrap();
    assert!(session.events.iter().any(|event| {
        event
            .attributes
            .get("merged_subagent")
            .and_then(|value| value.as_bool())
            == Some(true)
    }));
    assert!(session.events.iter().any(|event| {
        matches!(event.event_type, EventType::TaskStart { .. })
            && event
                .attributes
                .get("subagent_id")
                .and_then(|value| value.as_str())
                .is_some()
    }));
    assert!(session.events.iter().any(|event| {
        matches!(event.event_type, EventType::AgentMessage)
            && event.content.blocks.iter().any(|block| {
                matches!(block, opensession_core::trace::ContentBlock::Text { text } if text.contains("sibling subagent reply"))
            })
    }));
}

#[test]
fn test_parent_id_meta_marks_main_parser_session_as_auxiliary() {
    let dir = test_temp_root();
    let path = dir
        .as_path()
        .join("70dafb43-dbdd-4009-beb0-b6ac2bd9c4d1.jsonl");
    let entry = serde_json::json!({
        "type": "assistant",
        "uuid": "a1",
        "sessionId": "subagent-random",
        "parentId": "parent-main",
        "timestamp": Utc::now().to_rfc3339(),
        "message": {
            "role": "assistant",
            "model": "claude-3-opus",
            "content": [{
                "type": "text",
                "text": "sub"
            }]
        }
    })
    .to_string();
    write(&path, entry).unwrap();

    let parsed = parse_claude_code_jsonl(&path).unwrap();
    assert_eq!(
        parsed
            .context
            .attributes
            .get("session_role")
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert_eq!(
        parsed
            .context
            .attributes
            .get("parent_session_id")
            .and_then(|value| value.as_str()),
        Some("parent-main")
    );
    assert_eq!(
        parsed.context.related_session_ids,
        vec!["parent-main".to_string()]
    );
}

#[test]
fn test_subagent_meta_reads_parent_uuid_aliases() {
    let dir = test_temp_root();
    let subagent_path = dir.as_path().join("agent-xyz.jsonl");
    let subagent_entry = serde_json::json!({
        "type": "assistant",
        "uuid": "a1",
        "sessionId": "sub-1",
        "timestamp": Utc::now().to_rfc3339(),
        "parentId": "parent-1",
        "message": {
            "role": "assistant",
            "model": "claude-3-opus",
            "content": [{
                "type": "text",
                "text": "sub"
            }]
        }
    })
    .to_string();
    write(&subagent_path, subagent_entry).unwrap();

    let meta = read_subagent_meta(&subagent_path).unwrap();
    assert_eq!(meta.parent_session_id.as_deref(), Some("parent-1"));
}

#[test]
fn test_subagent_parse_sets_related_parent_session_id() {
    let dir = test_temp_root();
    let subagent_path = dir.as_path().join("agent-related.jsonl");
    let subagent_entry = serde_json::json!({
        "type": "assistant",
        "uuid": "a1",
        "sessionId": "sub-2",
        "timestamp": Utc::now().to_rfc3339(),
        "parentId": "parent-2",
        "message": {
            "role": "assistant",
            "model": "claude-3-opus",
            "content": [{
                "type": "text",
                "text": "sub"
            }]
        }
    })
    .to_string();
    write(&subagent_path, subagent_entry).unwrap();

    let parsed = super::super::subagent::parse_subagent_jsonl(&subagent_path).unwrap();
    assert_eq!(
        parsed.context.related_session_ids,
        vec!["parent-2".to_string()]
    );
    assert_eq!(
        parsed
            .context
            .attributes
            .get("session_role")
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert_eq!(
        parsed
            .context
            .attributes
            .get("parent_session_id")
            .and_then(|value| value.as_str()),
        Some("parent-2")
    );
}
