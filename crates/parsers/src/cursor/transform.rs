use opensession_core::trace::{Content, ContentBlock, EventType};

// ── Tool name mapping ──────────────────────────────────────────────────────

/// Resolve a Cursor tool numeric ID to a human-readable name.
/// Falls back to the string `name` field if present, otherwise returns
/// a generic name based on the numeric ID.
pub(super) fn resolve_tool_name(tool_id: Option<u32>, name: Option<&str>) -> String {
    if let Some(id) = tool_id {
        match id {
            3 => "grep_search".to_string(),
            5 => "read_file".to_string(),
            6 => "list_dir".to_string(),
            7 => "edit_file".to_string(),
            8 => "file_search".to_string(),
            12 => "reapply".to_string(),
            15 => "run_terminal_cmd".to_string(),
            18 => "web_search".to_string(),
            _ => name
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("tool_{}", id)),
        }
    } else {
        name.map(|n| n.to_string())
            .unwrap_or_else(|| "unknown_tool".to_string())
    }
}

/// Classify a resolved tool name into the appropriate HAIL EventType for a ToolCall.
pub(super) fn classify_cursor_tool(tool_name: &str, args: &serde_json::Value) -> EventType {
    match tool_name {
        "edit_file" | "reapply" => {
            let path = args
                .get("target_file")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "read_file" => {
            let path = args
                .get("target_file")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        "list_dir" => {
            let path = args
                .get("relative_workspace_path")
                .or_else(|| args.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            EventType::ToolCall {
                name: format!("list_dir: {}", path),
            }
        }
        "run_terminal_cmd" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::ShellCommand {
                command,
                exit_code: None,
            }
        }
        "grep_search" => {
            let query = args
                .get("query")
                .or_else(|| args.get("search_term"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::CodeSearch { query }
        }
        "file_search" => {
            let pattern = args
                .get("query")
                .or_else(|| args.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            EventType::FileSearch { pattern }
        }
        "web_search" => {
            let query = args
                .get("query")
                .or_else(|| args.get("search_query"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::WebSearch { query }
        }
        _ => EventType::ToolCall {
            name: tool_name.to_string(),
        },
    }
}

/// Build ToolCall content from tool args JSON
pub(super) fn tool_call_content(tool_name: &str, args: &serde_json::Value) -> Content {
    match tool_name {
        "edit_file" | "reapply" => {
            let path = args
                .get("target_file")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let mut blocks = vec![ContentBlock::Text {
                text: path.to_string(),
            }];
            if let Some(edit) = args.get("code_edit").and_then(|v| v.as_str()) {
                if !edit.is_empty() {
                    blocks.push(ContentBlock::Code {
                        code: edit.to_string(),
                        language: crate::common::detect_language(path),
                        start_line: None,
                    });
                }
            }
            Content { blocks }
        }
        "read_file" => {
            let path = args
                .get("target_file")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "run_terminal_cmd" => {
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            Content {
                blocks: vec![ContentBlock::Code {
                    code: command.to_string(),
                    language: Some("bash".to_string()),
                    start_line: None,
                }],
            }
        }
        "grep_search" => {
            let query = args
                .get("query")
                .or_else(|| args.get("search_term"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Content::text(query)
        }
        "file_search" => {
            let pattern = args
                .get("query")
                .or_else(|| args.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("*");
            Content::text(pattern)
        }
        "list_dir" => {
            let path = args
                .get("relative_workspace_path")
                .or_else(|| args.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            Content::text(path)
        }
        "web_search" => {
            let query = args
                .get("query")
                .or_else(|| args.get("search_query"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Content::text(query)
        }
        _ => {
            if args.is_null() || (args.is_object() && args.as_object().unwrap().is_empty()) {
                Content::empty()
            } else {
                Content {
                    blocks: vec![ContentBlock::Json { data: args.clone() }],
                }
            }
        }
    }
}

/// Parse a tool result string into structured Content.
/// The result is typically JSON; we try to extract useful diff/output information.
pub(super) fn parse_tool_result(tool_name: &str, result_str: &str) -> Content {
    // Try to parse as JSON first
    if let Ok(result_json) = serde_json::from_str::<serde_json::Value>(result_str) {
        match tool_name {
            "edit_file" | "reapply" => {
                // Extract diff info if available
                if let Some(diff) = result_json.get("diff") {
                    let is_applied = result_json
                        .get("isApplied")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let mut blocks = Vec::new();
                    if is_applied {
                        blocks.push(ContentBlock::Text {
                            text: "Applied".to_string(),
                        });
                    }
                    blocks.push(ContentBlock::Json { data: diff.clone() });
                    return Content { blocks };
                }
            }
            "run_terminal_cmd" => {
                // Extract command output
                if let Some(output) = result_json.get("output").and_then(|v| v.as_str()) {
                    return Content {
                        blocks: vec![ContentBlock::Code {
                            code: output.to_string(),
                            language: Some("text".to_string()),
                            start_line: None,
                        }],
                    };
                }
            }
            _ => {}
        }

        // Fallback: return JSON content
        Content {
            blocks: vec![ContentBlock::Json { data: result_json }],
        }
    } else if !result_str.trim().is_empty() {
        // Plain text result
        Content::text(result_str.trim())
    } else {
        Content::empty()
    }
}

// ── Model inference helpers ────────────────────────────────────────────────

/// Try to extract a model name from a thinking block's signature string.
/// Cursor signatures may encode model info.
pub(super) fn extract_model_from_signature(signature: &str) -> Option<String> {
    // Signatures that look like base64 tokens (e.g., "ADAxMjO3EpQ...") should
    // not be treated as model names. Skip if the signature is mostly
    // base64-like characters.
    if signature.len() > 30
        || signature.contains('=')
        || signature.contains('+')
        || signature.contains('/')
    {
        return None;
    }

    // Common patterns in signatures that hint at the model
    let lower = signature.to_lowercase();
    if lower.contains("claude") {
        // Try to find a specific Claude model version
        if lower.contains("opus") {
            Some("claude-opus".to_string())
        } else if lower.contains("sonnet") {
            Some("claude-sonnet".to_string())
        } else if lower.contains("haiku") {
            Some("claude-haiku".to_string())
        } else {
            Some("claude".to_string())
        }
    } else if lower.contains("gpt-4") {
        Some("gpt-4".to_string())
    } else if lower.starts_with("o1") || lower.starts_with("o3") {
        Some(lower.clone())
    } else {
        None
    }
}

/// Infer the AI provider from the model name.
pub(super) fn infer_provider(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower.contains("claude") || lower.contains("anthropic") {
        "anthropic".to_string()
    } else if lower.contains("gpt") || lower.contains("o1") || lower.contains("o3") {
        "openai".to_string()
    } else if lower.contains("gemini") {
        "google".to_string()
    } else if lower.contains("llama") || lower.contains("codellama") {
        "meta".to_string()
    } else if lower.contains("deepseek") {
        "deepseek".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_tool_name_known_ids() {
        assert_eq!(resolve_tool_name(Some(7), None), "edit_file");
        assert_eq!(resolve_tool_name(Some(5), None), "read_file");
        assert_eq!(resolve_tool_name(Some(6), None), "list_dir");
        assert_eq!(resolve_tool_name(Some(15), None), "run_terminal_cmd");
        assert_eq!(resolve_tool_name(Some(8), None), "file_search");
        assert_eq!(resolve_tool_name(Some(3), None), "grep_search");
        assert_eq!(resolve_tool_name(Some(18), None), "web_search");
        assert_eq!(resolve_tool_name(Some(12), None), "reapply");
    }

    #[test]
    fn test_resolve_tool_name_unknown_id_with_name() {
        assert_eq!(
            resolve_tool_name(Some(99), Some("custom_tool")),
            "custom_tool"
        );
    }

    #[test]
    fn test_resolve_tool_name_no_id() {
        assert_eq!(resolve_tool_name(None, Some("manual_name")), "manual_name");
        assert_eq!(resolve_tool_name(None, None), "unknown_tool");
    }

    #[test]
    fn test_classify_cursor_tool_edit() {
        let args = serde_json::json!({"target_file": "/tmp/test.rs", "code_edit": "fn main() {}"});
        let et = classify_cursor_tool("edit_file", &args);
        match et {
            EventType::FileEdit { path, .. } => assert_eq!(path, "/tmp/test.rs"),
            _ => panic!("Expected FileEdit"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_read() {
        let args = serde_json::json!({"target_file": "/tmp/test.rs"});
        let et = classify_cursor_tool("read_file", &args);
        match et {
            EventType::FileRead { path } => assert_eq!(path, "/tmp/test.rs"),
            _ => panic!("Expected FileRead"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_shell() {
        let args = serde_json::json!({"command": "cargo test"});
        let et = classify_cursor_tool("run_terminal_cmd", &args);
        match et {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "cargo test"),
            _ => panic!("Expected ShellCommand"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_grep() {
        let args = serde_json::json!({"query": "fn main"});
        let et = classify_cursor_tool("grep_search", &args);
        match et {
            EventType::CodeSearch { query } => assert_eq!(query, "fn main"),
            _ => panic!("Expected CodeSearch"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_file_search() {
        let args = serde_json::json!({"query": "config"});
        let et = classify_cursor_tool("file_search", &args);
        match et {
            EventType::FileSearch { pattern } => assert_eq!(pattern, "config"),
            _ => panic!("Expected FileSearch"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_web_search() {
        let args = serde_json::json!({"query": "rust async"});
        let et = classify_cursor_tool("web_search", &args);
        match et {
            EventType::WebSearch { query } => assert_eq!(query, "rust async"),
            _ => panic!("Expected WebSearch"),
        }
    }

    #[test]
    fn test_classify_cursor_tool_unknown() {
        let args = serde_json::json!({});
        let et = classify_cursor_tool("some_custom_tool", &args);
        match et {
            EventType::ToolCall { name } => assert_eq!(name, "some_custom_tool"),
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider("claude-3.5-sonnet"), "anthropic");
        assert_eq!(infer_provider("gpt-4o"), "openai");
        assert_eq!(infer_provider("gemini-pro"), "google");
        assert_eq!(infer_provider("deepseek-coder"), "deepseek");
        assert_eq!(infer_provider("unknown-model"), "unknown");
    }

    #[test]
    fn test_extract_model_from_signature() {
        assert_eq!(
            extract_model_from_signature("claude-sonnet-abc123"),
            Some("claude-sonnet".to_string())
        );
        assert_eq!(extract_model_from_signature("random-string"), None);
        // Base64 signature tokens should be rejected
        assert_eq!(
            extract_model_from_signature(
                "ADAxMjO3EpQziZkezZUa8dpFRYOY82GboyYhlgE8AX0bIJ5s0fSC4liZZCCYAkpHubsGBhdLlrDWSQ=="
            ),
            None,
        );
        // Short base64 with = padding
        assert_eq!(extract_model_from_signature("ADAxMjOS90fxYNmbz7C="), None,);
    }

    #[test]
    fn test_parse_tool_result_edit() {
        let result = r#"{"diff":{"added":5,"removed":2},"isApplied":true}"#;
        let content = parse_tool_result("edit_file", result);
        assert!(!content.blocks.is_empty());
        match &content.blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Applied"),
            _ => panic!("Expected Text 'Applied' block"),
        }
    }

    #[test]
    fn test_parse_tool_result_plain_text() {
        let content = parse_tool_result("some_tool", "plain output");
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "plain output"),
            _ => panic!("Expected Text block"),
        }
    }
}
