use crate::common::{build_tool_result_content, strip_system_reminders, ToolUseInfo};
use crate::SessionParser;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

pub struct ClaudeCodeParser;

impl SessionParser for ClaudeCodeParser {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "jsonl")
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_claude_code_jsonl(path)
    }
}

// ── Raw JSONL deserialization types ──────────────────────────────────────────

/// Top-level entry in the Claude Code JSONL file.
/// Each line is one of these.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum RawEntry {
    #[serde(rename = "user")]
    User(RawConversationEntry),
    #[serde(rename = "assistant")]
    Assistant(RawConversationEntry),
    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {},
    // Catch-all for unknown types we want to skip
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConversationEntry {
    uuid: String,
    #[serde(default)]
    session_id: Option<String>,
    timestamp: String,
    message: RawMessage,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RawMessage {
    role: String,
    content: RawContent,
    #[serde(default)]
    model: Option<String>,
}

/// Claude Code represents user message content as either a plain string
/// or an array of content blocks.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawContent {
    Text(String),
    Blocks(Vec<RawContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum RawContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
    },
    #[serde(rename = "thinking")]
    Thinking {
        #[serde(default)]
        thinking: Option<String>,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    // Skip unknown block types gracefully
    #[serde(other)]
    Other,
}

/// tool_result content can be a string, array of blocks, or absent
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultBlock>),
    #[default]
    Null,
}


#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ToolResultBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

// ── Content transformation helpers ──────────────────────────────────────────

/// Extract raw text from ToolResult content
fn tool_result_content_to_string(content: &ToolResultContent) -> String {
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
fn build_cc_tool_result_content(
    raw_content: &ToolResultContent,
    tool_info: &ToolUseInfo,
) -> Content {
    let raw_text = tool_result_content_to_string(raw_content);
    build_tool_result_content(&raw_text, tool_info)
}

// ── Parsing logic ───────────────────────────────────────────────────────────

fn parse_claude_code_jsonl(path: &Path) -> Result<Session> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open JSONL file: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);

    let mut events: Vec<Event> = Vec::new();
    let mut model_name: Option<String> = None;
    let mut tool_version: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;

    // Map tool_use_id -> tool metadata (name + file_path for language detection)
    let mut tool_use_info: HashMap<String, ToolUseInfo> = HashMap::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("Failed to read JSONL line: {}", e);
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!("Skipping unparseable JSONL line: {}", e);
                continue;
            }
        };

        match entry {
            RawEntry::FileHistorySnapshot {} | RawEntry::Unknown => continue,
            RawEntry::User(conv) => {
                if session_id.is_none() {
                    session_id = conv.session_id.clone();
                }
                if tool_version.is_none() {
                    tool_version = conv.version.clone();
                }
                if cwd.is_none() {
                    cwd = conv.cwd.clone();
                }
                let ts = parse_timestamp(&conv.timestamp)?;
                process_user_entry(
                    &conv,
                    ts,
                    &mut events,
                    &tool_use_info,
                );
            }
            RawEntry::Assistant(conv) => {
                if session_id.is_none() {
                    session_id = conv.session_id.clone();
                }
                if tool_version.is_none() {
                    tool_version = conv.version.clone();
                }
                if model_name.is_none() {
                    model_name = conv.message.model.clone();
                }
                let ts = parse_timestamp(&conv.timestamp)?;
                process_assistant_entry(
                    &conv,
                    ts,
                    &mut events,
                    &mut tool_use_info,
                );
            }
        }
    }

    // Derive session_id from file name if not found in entries
    let session_id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let agent = Agent {
        provider: "anthropic".to_string(),
        model: model_name.unwrap_or_else(|| "unknown".to_string()),
        tool: "claude-code".to_string(),
        tool_version,
    };

    let (created_at, updated_at) = if let (Some(first), Some(last)) =
        (events.first(), events.last())
    {
        (first.timestamp, last.timestamp)
    } else {
        let now = Utc::now();
        (now, now)
    };

    let mut attributes = HashMap::new();
    if let Some(ref dir) = cwd {
        attributes.insert(
            "cwd".to_string(),
            serde_json::Value::String(dir.clone()),
        );
    }

    let context = SessionContext {
        title: None,
        description: None,
        tags: vec!["claude-code".to_string()],
        created_at,
        updated_at,
        attributes,
    };

    let mut session = Session::new(session_id, agent);
    session.context = context;
    session.events = events;
    session.recompute_stats();

    Ok(session)
}

fn parse_timestamp(ts: &str) -> Result<DateTime<Utc>> {
    // Claude Code timestamps are ISO 8601, e.g. "2026-02-06T04:46:17.839Z"
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Fallback: try parsing without timezone
            chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|ndt| ndt.and_utc())
        })
        .with_context(|| format!("Failed to parse timestamp: {}", ts))
}

fn process_user_entry(
    conv: &RawConversationEntry,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    tool_use_info: &HashMap<String, ToolUseInfo>,
) {
    match &conv.message.content {
        RawContent::Text(text) => {
            let cleaned = strip_system_reminders(text);
            if !cleaned.trim().is_empty() {
                events.push(Event {
                    event_id: conv.uuid.clone(),
                    timestamp: ts,
                    event_type: EventType::UserMessage,
                    task_id: None,
                    content: Content::text(cleaned),
                    duration_ms: None,
                    attributes: HashMap::new(),
                });
            }
        }
        RawContent::Blocks(blocks) => {
            for block in blocks {
                match block {
                    RawContentBlock::Text { text } => {
                        let cleaned = strip_system_reminders(text);
                        if !cleaned.trim().is_empty() {
                            events.push(Event {
                                event_id: format!("{}-text", conv.uuid),
                                timestamp: ts,
                                event_type: EventType::UserMessage,
                                task_id: None,
                                content: Content::text(cleaned),
                                duration_ms: None,
                                attributes: HashMap::new(),
                            });
                        }
                    }
                    RawContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let info = tool_use_info
                            .get(tool_use_id)
                            .cloned()
                            .unwrap_or_else(|| ToolUseInfo {
                                name: "unknown".to_string(),
                                file_path: None,
                            });

                        let tool_name = info.name.clone();
                        let result_content = build_cc_tool_result_content(content, &info);

                        events.push(Event {
                            event_id: format!("{}-result-{}", conv.uuid, tool_use_id),
                            timestamp: ts,
                            event_type: EventType::ToolResult {
                                name: tool_name,
                                is_error: *is_error,
                                call_id: Some(tool_use_id.clone()),
                            },
                            task_id: None,
                            content: result_content,
                            duration_ms: None,
                            attributes: HashMap::new(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

fn process_assistant_entry(
    conv: &RawConversationEntry,
    ts: DateTime<Utc>,
    events: &mut Vec<Event>,
    tool_use_info: &mut HashMap<String, ToolUseInfo>,
) {
    // Build per-event attributes with model info
    let mut attrs = HashMap::new();
    if let Some(ref model) = conv.message.model {
        attrs.insert(
            "model".to_string(),
            serde_json::Value::String(model.clone()),
        );
    }

    if let RawContent::Blocks(blocks) = &conv.message.content {
        for block in blocks {
            match block {
                RawContentBlock::Text { text } => {
                    let cleaned = strip_system_reminders(text);
                    if cleaned.is_empty() {
                        continue;
                    }
                    events.push(Event {
                        event_id: format!("{}-text", conv.uuid),
                        timestamp: ts,
                        event_type: EventType::AgentMessage,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: attrs.clone(),
                    });
                }
                RawContentBlock::Thinking { thinking } => {
                    let text = thinking.as_deref().unwrap_or("");
                    let cleaned = strip_system_reminders(text);
                    if cleaned.is_empty() {
                        continue;
                    }
                    events.push(Event {
                        event_id: format!("{}-thinking", conv.uuid),
                        timestamp: ts,
                        event_type: EventType::Thinking,
                        task_id: None,
                        content: Content::text(cleaned),
                        duration_ms: None,
                        attributes: attrs.clone(),
                    });
                }
                RawContentBlock::ToolUse { id, name, input } => {
                    // Extract file_path from tool input for language detection in ToolResult
                    let file_path = match name.as_str() {
                        "Read" | "Write" | "Edit" | "NotebookEdit" => input
                            .get("file_path")
                            .or_else(|| input.get("notebook_path"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        "Grep" => input
                            .get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    };

                    // Track tool_use_id -> info for matching ToolResults
                    if let Some(tool_id) = id {
                        tool_use_info.insert(
                            tool_id.clone(),
                            ToolUseInfo {
                                name: name.clone(),
                                file_path,
                            },
                        );
                    }

                    let event_type = classify_tool_use(name, input);
                    let content = tool_use_content(name, input);

                    events.push(Event {
                        event_id: id
                            .clone()
                            .unwrap_or_else(|| format!("{}-tool", conv.uuid)),
                        timestamp: ts,
                        event_type,
                        task_id: None,
                        content,
                        duration_ms: None,
                        attributes: attrs.clone(),
                    });
                }
                _ => {}
            }
        }
    }
}

/// Classify a tool_use block into a specific HAIL EventType.
/// Maps well-known Claude Code tools to semantic event types.
fn classify_tool_use(name: &str, input: &serde_json::Value) -> EventType {
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
fn tool_use_content(name: &str, input: &serde_json::Value) -> Content {
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
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("*");
            Content::text(pattern)
        }
        "Grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Content::text(pattern)
        }
        "Task" => {
            // Sub-agent: extract description and prompt as separate text blocks
            let mut blocks = Vec::new();
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
    use super::*;

    #[test]
    fn test_parse_timestamp() {
        let ts = parse_timestamp("2026-02-06T04:46:17.839Z").unwrap();
        assert_eq!(ts.year(), 2026);
    }

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
    fn test_raw_entry_deserialization_user_string() {
        let json = r#"{"type":"user","uuid":"abc","sessionId":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let entry: RawEntry = serde_json::from_str(json).unwrap();
        match entry {
            RawEntry::User(conv) => {
                assert_eq!(conv.uuid, "abc");
                match conv.message.content {
                    RawContent::Text(t) => assert_eq!(t, "hello"),
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

    // ── Claude Code–specific content tests ────────────────────────────────

    #[test]
    fn test_cc_build_tool_result_content_read() {
        let raw = ToolResultContent::Text(
            "     1→use std::io;\n     2→fn main() {}".to_string(),
        );
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
    fn test_tool_use_content_task() {
        let input = serde_json::json!({
            "description": "Search for files",
            "prompt": "Find all TypeScript files"
        });
        let content = tool_use_content("Task", &input);
        assert_eq!(content.blocks.len(), 2);
        match &content.blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Search for files"),
            _ => panic!("Expected Text block"),
        }
    }

    use chrono::Datelike;
}
