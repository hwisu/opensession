use super::*;
use chrono::Datelike;
use std::fs::{create_dir_all, write};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_millis_to_datetime() {
    let dt = millis_to_datetime(1753359830903);
    assert!(dt.year() >= 2025);
}

#[test]
fn test_classify_bash() {
    let input = Some(serde_json::json!({"command": "ls -la"}));
    let et = classify_opencode_tool("bash", &input);
    match et {
        EventType::ShellCommand { command, .. } => assert_eq!(command, "ls -la"),
        _ => panic!("Expected ShellCommand"),
    }
}

#[test]
fn test_classify_read_with_camel_case_path() {
    let input = Some(serde_json::json!({"filePath": "/tmp/demo.rs"}));
    let et = classify_opencode_tool("read", &input);
    match et {
        EventType::FileRead { path } => assert_eq!(path, "/tmp/demo.rs"),
        _ => panic!("Expected FileRead"),
    }
}

#[test]
fn test_tool_status_terminal_variants() {
    assert!(is_terminal_tool_status("completed"));
    assert!(is_terminal_tool_status("FAILED"));
    assert!(is_terminal_tool_status("canceled"));
    assert!(!is_terminal_tool_status("running"));
}

#[test]
fn test_normalized_call_id_trims_whitespace() {
    assert_eq!(
        normalized_call_id(Some("  functions.edit:27 ")).as_deref(),
        Some("functions.edit:27")
    );
    assert_eq!(normalized_call_id(Some("   ")), None);
}

#[test]
fn test_extract_tool_output_text_fallbacks() {
    let state_output = ToolState {
        status: Some("completed".to_string()),
        input: None,
        output: Some(serde_json::json!("done")),
        error: Some(serde_json::json!("ignored error")),
        metadata: None,
        title: None,
        time: None,
    };
    assert_eq!(
        extract_tool_output_text(Some(&state_output)).as_deref(),
        Some("done")
    );

    let state_error = ToolState {
        status: Some("error".to_string()),
        input: None,
        output: None,
        error: Some(serde_json::json!("failed")),
        metadata: Some(serde_json::json!({"output": "metadata output"})),
        title: None,
        time: None,
    };
    assert_eq!(
        extract_tool_output_text(Some(&state_error)).as_deref(),
        Some("failed")
    );

    let state_meta = ToolState {
        status: Some("error".to_string()),
        input: None,
        output: None,
        error: None,
        metadata: Some(serde_json::json!({"output": "metadata output"})),
        title: None,
        time: None,
    };
    assert_eq!(
        extract_tool_output_text(Some(&state_meta)).as_deref(),
        Some("metadata output")
    );
}

#[test]
fn test_session_info_deser() {
    let json = r#"{"id":"ses_abc","version":"1.1.30","title":"Test session","projectID":"abc123","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
    let info: SessionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.id, "ses_abc");
    assert_eq!(info.title, Some("Test session".to_string()));
    assert_eq!(info.directory, Some("/tmp/proj".to_string()));
}

#[test]
fn test_session_info_parent_id_deser() {
    let json = r#"{"id":"ses_child","version":"1.1.30","parentID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
    let info: SessionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.parent_id, Some("ses_parent".to_string()));
}

#[test]
fn test_session_info_parent_id_alias_deser() {
    let json = r#"{"id":"ses_child","version":"1.1.30","parentId":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
    let info: SessionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.parent_id, Some("ses_parent".to_string()));
}

#[test]
fn test_session_info_parent_uuid_alias_deser() {
    let json = r#"{"id":"ses_child","version":"1.1.30","parentUUID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#;
    let info: SessionInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.parent_id, Some("ses_parent".to_string()));
}

#[test]
fn test_session_context_has_source_path() {
    let temp_dir = std::env::temp_dir().join(format!(
        "opensession-opencode-parser-source-path-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock ok")
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let session_path = temp_dir.join("session.json");
    write(
        &session_path,
        r#"{"id":"ses_parent","version":"1.1.30","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
    )
    .unwrap();

    let session = parse_opencode_session(&session_path).expect("parse session");
    assert_eq!(
        session
            .context
            .attributes
            .get("source_path")
            .and_then(|value| value.as_str()),
        Some(session_path.to_str().expect("path to str"))
    );
}

#[test]
fn test_message_info_deser() {
    let json = r#"{"id":"msg_abc","sessionID":"ses_abc","role":"user","model":{"providerID":"openai","modelID":"gpt-5.2-codex"},"time":{"created":1753359830903}}"#;
    let msg: MessageInfo = serde_json::from_str(json).unwrap();
    assert_eq!(msg.id, "msg_abc");
    assert_eq!(msg.role, "user");
    let model = msg.model.expect("model ref");
    assert_eq!(model.provider_id, Some("openai".to_string()));
    assert_eq!(model.model_id, Some("gpt-5.2-codex".to_string()));
}

#[test]
fn test_message_info_deser_top_level_model_fields() {
    let json = r#"{"id":"msg_xyz","sessionID":"ses_abc","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359830903}}"#;
    let msg: MessageInfo = serde_json::from_str(json).unwrap();
    assert_eq!(msg.id, "msg_xyz");
    assert_eq!(msg.provider_id.as_deref(), Some("openai"));
    assert_eq!(msg.model_id.as_deref(), Some("gpt-5.2-codex"));
}

#[test]
fn test_can_parse() {
    let parser = OpenCodeParser;
    assert!(parser.can_parse(Path::new(
        "/Users/test/.local/share/opencode/storage/session/abc123/ses_xyz.json"
    )));
    assert!(!parser.can_parse(Path::new(
        "/Users/test/.local/share/opencode/storage/message/ses_xyz/msg_abc.json"
    )));
}

fn tmp_test_root() -> std::path::PathBuf {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be valid")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("opensession-opencode-parser-{since_epoch}"));
    std::fs::create_dir_all(&root).expect("create temp dir");
    root
}

#[test]
fn test_parse_relates_child_session_to_parent() {
    let root = tmp_test_root();
    let project = root.join("proj-test");
    let session_dir = project.join("storage").join("session").join("example");
    let message_dir = project.join("storage").join("message").join("ses_child");
    let part_dir = project.join("storage").join("part").join("msg_001");
    create_dir_all(&session_dir).expect("create session dir");
    create_dir_all(&message_dir).expect("create message dir");
    create_dir_all(&part_dir).expect("create part dir");

    write(
        session_dir.join("ses_child.json"),
        r#"{"id":"ses_child","version":"1.1.30","parentID":"ses_parent","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
    )
    .expect("write session file");
    write(
        message_dir.join("msg_001.json"),
        r#"{"id":"msg_001","sessionID":"ses_child","role":"user","time":{"created":1753359831000}}"#,
    )
    .expect("write message file");
    write(
        part_dir.join("part_001.json"),
        r#"{"id":"part_001","messageID":"msg_001","type":"text","text":"hello","time":{"start":1753359831000,"end":1753359831000}}"#,
    )
    .expect("write part file");

    let session =
        parse_opencode_session(&session_dir.join("ses_child.json")).expect("parse session");
    assert_eq!(
        session.context.related_session_ids,
        vec!["ses_parent".to_string()]
    );
    assert_eq!(
        session
            .context
            .attributes
            .get("parent_session_id")
            .and_then(|value| value.as_str()),
        Some("ses_parent")
    );
    assert_eq!(
        session
            .context
            .attributes
            .get("session_role")
            .and_then(|value| value.as_str()),
        Some("auxiliary")
    );
    assert_eq!(session.stats.event_count, 1);
}

#[test]
fn test_parse_part_dir_prefixed_msg_fallback() {
    let root = tmp_test_root();
    let project = root.join("proj-prefixed");
    let session_dir = project.join("storage").join("session").join("example");
    let message_dir = project.join("storage").join("message").join("ses_fallback");
    let part_dir = project.join("storage").join("part").join("msg_abc123");
    create_dir_all(&session_dir).expect("create session dir");
    create_dir_all(&message_dir).expect("create message dir");
    create_dir_all(&part_dir).expect("create part dir");

    write(
        session_dir.join("ses_fallback.json"),
        r#"{"id":"ses_fallback","version":"1.1.30","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
    )
    .expect("write session file");
    write(
        message_dir.join("abc123.json"),
        r#"{"id":"abc123","sessionID":"ses_fallback","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359831000}}"#,
    )
    .expect("write message file");
    write(
        part_dir.join("part_001.json"),
        r#"{"id":"part_001","messageID":"abc123","type":"text","text":"assistant reply","time":{"start":1753359831000,"end":1753359831200}}"#,
    )
    .expect("write part file");

    let session =
        parse_opencode_session(&session_dir.join("ses_fallback.json")).expect("parse session");
    assert!(
        session
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::AgentMessage))
    );
    assert_eq!(session.agent.provider, "openai");
    assert_eq!(session.agent.model, "gpt-5.2-codex");
    assert_eq!(
        session
            .context
            .attributes
            .get("session_role")
            .and_then(|value| value.as_str()),
        Some("primary")
    );
}

#[test]
fn test_parse_reasoning_and_call_id_normalization() {
    let root = tmp_test_root();
    let project = root.join("proj-company");
    let session_dir = project.join("storage").join("session").join("example");
    let message_dir = project.join("storage").join("message").join("ses_company");
    let user_part_dir = project.join("storage").join("part").join("msg_user");
    let assistant_part_dir = project.join("storage").join("part").join("msg_assistant");
    create_dir_all(&session_dir).expect("create session dir");
    create_dir_all(&message_dir).expect("create message dir");
    create_dir_all(&user_part_dir).expect("create user part dir");
    create_dir_all(&assistant_part_dir).expect("create assistant part dir");

    write(
        session_dir.join("ses_company.json"),
        r#"{"id":"ses_company","version":"1.2.0","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
    )
    .expect("write session");
    write(
        message_dir.join("msg_user.json"),
        r#"{"id":"msg_user","sessionID":"ses_company","role":"user","model":{"providerID":"openai","modelID":"gpt-5.2-codex"},"time":{"created":1753359831000}}"#,
    )
    .expect("write user message");
    write(
        message_dir.join("msg_assistant.json"),
        r#"{"id":"msg_assistant","sessionID":"ses_company","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359832000,"completed":1753359835000}}"#,
    )
    .expect("write assistant message");

    write(
        user_part_dir.join("part_user_text.json"),
        r#"{"id":"part_user_text","messageID":"msg_user","type":"text","text":"run diagnostics","time":{"start":1753359831000,"end":1753359831100}}"#,
    )
    .expect("write user text part");
    write(
        user_part_dir.join("part_user_file.json"),
        r#"{"id":"part_user_file","messageID":"msg_user","type":"file","filename":"notes.md","url":"file:///tmp/proj/notes.md","time":{"start":1753359831050,"end":1753359831050}}"#,
    )
    .expect("write user file part");
    write(
        assistant_part_dir.join("part_reasoning.json"),
        r#"{"id":"part_reasoning","messageID":"msg_assistant","type":"reasoning","text":"","metadata":{"openai":{"reasoningEncryptedContent":"abc"}},"time":{"start":1753359832100,"end":1753359832200}}"#,
    )
    .expect("write reasoning part");
    write(
        assistant_part_dir.join("part_tool_done.json"),
        r#"{"id":"part_tool_done","messageID":"msg_assistant","type":"tool","callID":"call_abc","tool":"grep","state":{"status":"Completed","input":{"pattern":"todo","path":"/tmp/proj"},"output":"Found 1 match","title":"todo","time":{"start":1753359832300,"end":1753359832400}}}"#,
    )
    .expect("write completed tool part");
    write(
        assistant_part_dir.join("part_tool_running.json"),
        r#"{"id":"part_tool_running","messageID":"msg_assistant","type":"tool","callID":"  functions.edit:27 ","tool":"edit","state":{"status":"running","input":{"filePath":"/tmp/proj/main.rs"},"time":{"start":1753359832500}}}"#,
    )
    .expect("write running tool part");
    write(
        assistant_part_dir.join("part_patch.json"),
        r#"{"id":"part_patch","messageID":"msg_assistant","type":"patch","hash":"abc123","files":["/tmp/proj/main.rs","/tmp/proj/lib.rs"],"time":{"start":1753359832450,"end":1753359832450}}"#,
    )
    .expect("write patch part");

    let session =
        parse_opencode_session(&session_dir.join("ses_company.json")).expect("parse session");

    let thinking = session
        .events
        .iter()
        .find(|event| matches!(event.event_type, EventType::Thinking))
        .expect("thinking event");
    assert_eq!(
        thinking
            .content
            .blocks
            .first()
            .and_then(|block| match block {
                opensession_core::trace::ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            }),
        Some("Encrypted reasoning")
    );

    assert!(session.events.iter().any(|event| {
        matches!(
            &event.event_type,
            EventType::ToolResult { name, call_id, .. }
                if name == "grep" && call_id.as_deref() == Some("call_abc")
        )
    }));

    assert!(session.events.iter().any(|event| {
        matches!(event.event_type, EventType::UserMessage)
            && event.content.blocks.iter().any(|block| {
                matches!(
                    block,
                    opensession_core::trace::ContentBlock::Text { text } if text == "Attached file: notes.md"
                )
            })
    }));

    assert!(session.events.iter().any(|event| {
        matches!(&event.event_type, EventType::FileEdit { path, .. } if path == "/tmp/proj/lib.rs")
            && event
                .attributes
                .get("source.raw_type")
                .and_then(|value| value.as_str())
                == Some("part:patch:file")
    }));

    assert!(session.events.iter().any(|event| {
        matches!(&event.event_type, EventType::FileEdit { .. })
            && event
                .attributes
                .get("semantic.call_id")
                .and_then(|value| value.as_str())
                == Some("functions.edit:27")
    }));

    assert!(session.events.iter().any(|event| {
        matches!(&event.event_type, EventType::TaskEnd { .. })
            && event.task_id.as_deref() == Some("functions.edit:27")
            && event
                .attributes
                .get("source.raw_type")
                .and_then(|value| value.as_str())
                == Some("synthetic:task-end")
    }));
}

#[test]
fn test_patch_with_many_files_emits_summary_event() {
    let root = tmp_test_root();
    let project = root.join("proj-patch-summary");
    let session_dir = project.join("storage").join("session").join("example");
    let message_dir = project
        .join("storage")
        .join("message")
        .join("ses_patch_summary");
    let part_dir = project.join("storage").join("part").join("msg_assistant");
    create_dir_all(&session_dir).expect("create session dir");
    create_dir_all(&message_dir).expect("create message dir");
    create_dir_all(&part_dir).expect("create part dir");

    write(
        session_dir.join("ses_patch_summary.json"),
        r#"{"id":"ses_patch_summary","version":"1.2.0","directory":"/tmp/proj","time":{"created":1753359830903,"updated":1753360246507}}"#,
    )
    .expect("write session");
    write(
        message_dir.join("msg_assistant.json"),
        r#"{"id":"msg_assistant","sessionID":"ses_patch_summary","role":"assistant","providerID":"openai","modelID":"gpt-5.2-codex","time":{"created":1753359832000,"completed":1753359835000}}"#,
    )
    .expect("write assistant message");
    write(
        part_dir.join("part_patch_many.json"),
        r#"{"id":"part_patch_many","messageID":"msg_assistant","type":"patch","hash":"manyhash","files":["/tmp/proj/f1.rs","/tmp/proj/f2.rs","/tmp/proj/f3.rs","/tmp/proj/f4.rs","/tmp/proj/f5.rs","/tmp/proj/f6.rs","/tmp/proj/f7.rs","/tmp/proj/f8.rs","/tmp/proj/f9.rs"],"time":{"start":1753359832100,"end":1753359832200}}"#,
    )
    .expect("write patch part");

    let session = parse_opencode_session(&session_dir.join("ses_patch_summary.json"))
        .expect("parse patch summary session");

    assert!(
        !session
            .events
            .iter()
            .any(|event| matches!(&event.event_type, EventType::FileEdit { .. }))
    );
    assert!(session.events.iter().any(|event| {
        matches!(&event.event_type, EventType::Custom { kind } if kind == "patch")
            && event
                .attributes
                .get("source.raw_type")
                .and_then(|value| value.as_str())
                == Some("part:patch:summary")
            && event.content.blocks.iter().any(|block| {
                matches!(
                    block,
                    opensession_core::trace::ContentBlock::Json { data }
                        if data.get("file_count").and_then(|value| value.as_u64()) == Some(9)
                )
            })
    }));
}
