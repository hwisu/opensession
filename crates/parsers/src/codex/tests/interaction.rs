use super::*;

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
    let line = r#"{"type":"reasoning","id":"rs_456","summary":[],"encrypted_content":"gAAAAA..."}"#;
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
