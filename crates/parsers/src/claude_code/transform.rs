use crate::common::{build_tool_result_content, ToolUseInfo};
use opensession_core::trace::{Content, ContentBlock, EventType};

// ── Content transformation helpers ──────────────────────────────────────────

/// Extract raw text from ToolResult content
pub(super) fn tool_result_content_to_string(content: &super::parse::ToolResultContent) -> String {
    use super::parse::{ToolResultBlock, ToolResultContent};
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                if let ToolResultBlock::Text { text } = block {
                    parts.push(text.clone());
                }
            }
            parts.join("\n")
        }
        ToolResultContent::Null => String::new(),
    }
}

/// Build structured Content for a ToolResult event (delegates to common helper).
pub(super) fn build_cc_tool_result_content(
    raw_content: &super::parse::ToolResultContent,
    tool_info: &ToolUseInfo,
) -> Content {
    let raw_text = tool_result_content_to_string(raw_content);
    build_tool_result_content(&raw_text, tool_info)
}

// ── Tool classification ─────────────────────────────────────────────────────

/// Classify a tool_use block into a specific HAIL EventType.
/// Maps well-known Claude Code tools to semantic event types.
pub(super) fn classify_tool_use(name: &str, input: &serde_json::Value) -> EventType {
    match name {
        "Read" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileRead { path }
        }
        "Grep" => {
            let query = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::CodeSearch { query }
        }
        "Glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            EventType::FileSearch { pattern }
        }
        "Write" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileCreate { path }
        }
        "Edit" | "NotebookEdit" => {
            let path = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            EventType::FileEdit { path, diff: None }
        }
        "Bash" => {
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::ShellCommand {
                command,
                exit_code: None,
            }
        }
        "WebSearch" => {
            let query = input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::WebSearch { query }
        }
        "WebFetch" => {
            let url = input
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            EventType::WebFetch { url }
        }
        _ => EventType::ToolCall {
            name: name.to_string(),
        },
    }
}

/// Build content for a tool_use event.
/// Extracts the most useful information from the tool input
/// so the frontend can render without parsing raw JSON.
pub(super) fn tool_use_content(name: &str, input: &serde_json::Value) -> Content {
    match name {
        "Read" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "Write" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "Edit" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Content::text(path)
        }
        "Bash" => {
            let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let desc = input
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if desc.is_empty() {
                Content {
                    blocks: vec![ContentBlock::Code {
                        code: command.to_string(),
                        language: Some("bash".to_string()),
                        start_line: None,
                    }],
                }
            } else {
                Content {
                    blocks: vec![
                        ContentBlock::Text {
                            text: desc.to_string(),
                        },
                        ContentBlock::Code {
                            code: command.to_string(),
                            language: Some("bash".to_string()),
                            start_line: None,
                        },
                    ],
                }
            }
        }
        "Glob" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
            Content::text(pattern)
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            Content::text(pattern)
        }
        "Task" => {
            // Sub-agent: extract metadata, description, and prompt
            let mut blocks = Vec::new();

            // Build a header line with agent metadata
            let mut meta_parts = Vec::new();
            if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
                meta_parts.push(format!("agent: {}", name));
            }
            if let Some(agent_type) = input.get("subagent_type").and_then(|v| v.as_str()) {
                meta_parts.push(format!("type: {}", agent_type));
            }
            if let Some(team) = input.get("team_name").and_then(|v| v.as_str()) {
                meta_parts.push(format!("team: {}", team));
            }
            if let Some(mode) = input.get("mode").and_then(|v| v.as_str()) {
                meta_parts.push(format!("mode: {}", mode));
            }
            if input.get("run_in_background").and_then(|v| v.as_bool()) == Some(true) {
                meta_parts.push("background".to_string());
            }
            if !meta_parts.is_empty() {
                blocks.push(ContentBlock::Text {
                    text: format!("[{}]", meta_parts.join(", ")),
                });
            }

            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                if !desc.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: desc.to_string(),
                    });
                }
            }
            if let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) {
                if !prompt.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: prompt.to_string(),
                    });
                }
            }
            if blocks.is_empty() {
                blocks.push(ContentBlock::Json {
                    data: input.clone(),
                });
            }
            Content { blocks }
        }
        _ => Content {
            blocks: vec![ContentBlock::Json {
                data: input.clone(),
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse::ToolResultContent;
    use super::*;
    use crate::common::ToolUseInfo;

    #[test]
    fn test_classify_tool_use_read() {
        let input = serde_json::json!({"file_path": "/tmp/test.rs"});
        let event_type = classify_tool_use("Read", &input);
        match event_type {
            EventType::FileRead { path } => assert_eq!(path, "/tmp/test.rs"),
            _ => panic!("Expected FileRead"),
        }
    }

    #[test]
    fn test_classify_tool_use_grep() {
        let input = serde_json::json!({"pattern": "fn main", "path": "/tmp"});
        let event_type = classify_tool_use("Grep", &input);
        match event_type {
            EventType::CodeSearch { query } => assert_eq!(query, "fn main"),
            _ => panic!("Expected CodeSearch"),
        }
    }

    #[test]
    fn test_classify_tool_use_glob() {
        let input = serde_json::json!({"pattern": "**/*.rs"});
        let event_type = classify_tool_use("Glob", &input);
        match event_type {
            EventType::FileSearch { pattern } => assert_eq!(pattern, "**/*.rs"),
            _ => panic!("Expected FileSearch"),
        }
    }

    #[test]
    fn test_classify_tool_use_write() {
        let input = serde_json::json!({"file_path": "/tmp/new.rs", "content": "fn main() {}"});
        let event_type = classify_tool_use("Write", &input);
        match event_type {
            EventType::FileCreate { path } => assert_eq!(path, "/tmp/new.rs"),
            _ => panic!("Expected FileCreate"),
        }
    }

    #[test]
    fn test_classify_tool_use_edit() {
        let input =
            serde_json::json!({"file_path": "/tmp/test.rs", "old_string": "a", "new_string": "b"});
        let event_type = classify_tool_use("Edit", &input);
        match event_type {
            EventType::FileEdit { path, .. } => assert_eq!(path, "/tmp/test.rs"),
            _ => panic!("Expected FileEdit"),
        }
    }

    #[test]
    fn test_classify_tool_use_bash() {
        let input = serde_json::json!({"command": "cargo test"});
        let event_type = classify_tool_use("Bash", &input);
        match event_type {
            EventType::ShellCommand { command, .. } => assert_eq!(command, "cargo test"),
            _ => panic!("Expected ShellCommand"),
        }
    }

    #[test]
    fn test_tool_result_content_text() {
        let content = ToolResultContent::Text("output".to_string());
        assert_eq!(tool_result_content_to_string(&content), "output");
    }

    #[test]
    fn test_tool_result_content_blocks() {
        use super::super::parse::ToolResultBlock;
        let content = ToolResultContent::Blocks(vec![ToolResultBlock::Text {
            text: "line1".to_string(),
        }]);
        assert_eq!(tool_result_content_to_string(&content), "line1");
    }

    #[test]
    fn test_tool_result_content_null() {
        let content = ToolResultContent::Null;
        assert_eq!(tool_result_content_to_string(&content), "");
    }

    #[test]
    fn test_cc_build_tool_result_content_read() {
        let raw = ToolResultContent::Text("     1→use std::io;\n     2→fn main() {}".to_string());
        let info = ToolUseInfo {
            name: "Read".to_string(),
            file_path: Some("/tmp/test.rs".to_string()),
        };
        let content = build_cc_tool_result_content(&raw, &info);
        assert_eq!(content.blocks.len(), 1);
        match &content.blocks[0] {
            ContentBlock::Code {
                language,
                start_line,
                ..
            } => {
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(*start_line, Some(1));
            }
            _ => panic!("Expected Code block"),
        }
    }

    #[test]
    fn test_tool_use_content_task_simple() {
        let input = serde_json::json!({
            "description": "Search for files",
            "prompt": "Find all TypeScript files",
            "subagent_type": "Explore"
        });
        let content = tool_use_content("Task", &input);
        assert_eq!(content.blocks.len(), 3); // meta + desc + prompt
        match &content.blocks[0] {
            ContentBlock::Text { text } => assert!(text.contains("type: Explore")),
            _ => panic!("Expected Text block with metadata"),
        }
        match &content.blocks[1] {
            ContentBlock::Text { text } => assert_eq!(text, "Search for files"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_tool_use_content_task_team() {
        let input = serde_json::json!({
            "description": "Setup repo",
            "prompt": "Set up the repo...",
            "subagent_type": "general-purpose",
            "name": "agent-backend",
            "team_name": "dev-team",
            "mode": "bypassPermissions",
            "run_in_background": true
        });
        let content = tool_use_content("Task", &input);
        match &content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("agent: agent-backend"));
                assert!(text.contains("team: dev-team"));
                assert!(text.contains("background"));
            }
            _ => panic!("Expected metadata block"),
        }
    }
}
