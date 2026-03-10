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
    assert_eq!(
        session.messages[1]
            .thoughts
            .as_ref()
            .expect("thoughts")
            .len(),
        1
    );
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
            let tokens = tokens.expect("tokens");
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
    assert!(parsed.texts.is_empty());
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
        matches!(&event.event_type, EventType::ToolCall { name } if name == "run_shell_command")
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
            .and_then(|value| value.as_str())
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
    assert!(
        parsed
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::UserMessage))
    );
    assert!(
        parsed
            .events
            .iter()
            .any(|event| matches!(event.event_type, EventType::AgentMessage))
    );
    assert!(parsed.events.iter().all(|event| {
        event
            .attributes
            .get("source.schema_version")
            .and_then(|value| value.as_str())
            .is_some()
    }));
}
