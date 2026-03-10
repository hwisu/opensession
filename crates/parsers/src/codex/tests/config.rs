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
fn test_parse_function_output_recovers_meaningful_stdout_when_output_is_dot() {
    let raw = r#"{"output":".","stdout":"Session still running (pid=1234)","metadata":{"exit_code":0,"duration_seconds":0.01}}"#;
    let (text, is_error, duration) = parse_function_output(raw);
    assert_eq!(text, "Session still running (pid=1234)");
    assert!(!is_error);
    assert_eq!(duration, Some(10));
}

#[test]
fn test_parse_function_output_error() {
    let raw =
        r#"{"output":"command not found","metadata":{"exit_code":127,"duration_seconds":0.01}}"#;
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
