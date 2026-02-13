//! Incremental JSONL parser for Claude Code sessions.
//!
//! Converts individual JSONL lines into HAIL events without needing the full file.
//! Maintains state (tool_use_id -> info mapping) across lines.

use anyhow::Result;
use opensession_core::trace::{Agent, Event, SessionContext};
use std::collections::HashMap;

use crate::claude_code::{
    parse_timestamp, process_assistant_entry, process_user_entry, RawConversationEntry, RawEntry,
};
use crate::common::ToolUseInfo;

/// Incremental parser that maintains state across JSONL lines.
pub struct IncrementalParser {
    tool_use_info: HashMap<String, ToolUseInfo>,
    session_id: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    version: Option<String>,
}

impl Default for IncrementalParser {
    fn default() -> Self {
        Self::new()
    }
}

impl IncrementalParser {
    pub fn new() -> Self {
        Self {
            tool_use_info: HashMap::new(),
            session_id: None,
            model: None,
            cwd: None,
            version: None,
        }
    }

    /// Parse a single JSONL line into zero or more HAIL events.
    pub fn parse_line(&mut self, line: &str) -> Result<Vec<Event>> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(Vec::new());
        }

        let entry: RawEntry = serde_json::from_str(line)?;

        let mut events = Vec::new();

        match entry {
            RawEntry::User(conv) => {
                self.extract_metadata(&conv);
                let ts = parse_timestamp(&conv.timestamp)?;
                process_user_entry(&conv, ts, &mut events, &self.tool_use_info);
            }
            RawEntry::Assistant(conv) => {
                self.extract_metadata(&conv);
                let ts = parse_timestamp(&conv.timestamp)?;
                process_assistant_entry(&conv, ts, &mut events, &mut self.tool_use_info);
            }
            // Skip non-conversation entries
            _ => {}
        }

        Ok(events)
    }

    /// Extract session metadata from a conversation entry
    fn extract_metadata(&mut self, conv: &RawConversationEntry) {
        if self.session_id.is_none() {
            if let Some(ref sid) = conv.session_id {
                self.session_id = Some(String::clone(sid));
            }
        }
        if self.cwd.is_none() {
            if let Some(ref cwd) = conv.cwd {
                self.cwd = Some(String::clone(cwd));
            }
        }
        if self.version.is_none() {
            if let Some(ref ver) = conv.version {
                self.version = Some(String::clone(ver));
            }
        }
        if self.model.is_none() {
            if let Some(ref model) = conv.message.model {
                self.model = Some(String::clone(model));
            }
        }
    }

    /// Get the session ID if discovered
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Build session header info from discovered metadata.
    /// Returns None if not enough metadata has been seen yet.
    pub fn build_session_header(&self) -> Option<(Agent, SessionContext)> {
        let _session_id = self.session_id.as_ref()?;

        let agent = Agent {
            tool: "claude-code".to_string(),
            provider: "anthropic".to_string(),
            model: self.model.clone().unwrap_or_default(),
            tool_version: self.version.clone(),
        };

        let mut context = SessionContext::default();
        if let Some(ref cwd) = self.cwd {
            context.attributes.insert(
                "working_directory".to_string(),
                serde_json::Value::String(cwd.clone()),
            );
        }

        Some((agent, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_core::trace::EventType;

    #[test]
    fn test_parse_user_message() {
        let line = r#"{"type":"user","uuid":"u1","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello world"}}"#;
        let mut parser = IncrementalParser::new();
        let events = parser.parse_line(line).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::UserMessage));
    }

    #[test]
    fn test_parse_assistant_text() {
        let line = r#"{"type":"assistant","uuid":"a1","timestamp":"2024-01-01T00:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"Hi there"}],"model":"claude-sonnet-4-5-20250929"}}"#;
        let mut parser = IncrementalParser::new();
        let events = parser.parse_line(line).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::AgentMessage));
    }

    #[test]
    fn test_parse_tool_use() {
        let line = r#"{"type":"assistant","uuid":"a2","timestamp":"2024-01-01T00:00:02Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tu1","name":"Read","input":{"file_path":"/tmp/test.rs"}}],"model":"claude-sonnet-4-5-20250929"}}"#;
        let mut parser = IncrementalParser::new();
        let events = parser.parse_line(line).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event_type, EventType::FileRead { .. }));
    }

    #[test]
    fn test_session_metadata_extraction() {
        let line = r#"{"type":"user","uuid":"u1","sessionId":"sess-1","timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"Hello"},"cwd":"/home/user","version":"1.0.0"}"#;
        let mut parser = IncrementalParser::new();
        let _ = parser.parse_line(line).unwrap();
        assert_eq!(parser.session_id(), Some("sess-1"));
    }

    #[test]
    fn test_empty_line() {
        let mut parser = IncrementalParser::new();
        let events = parser.parse_line("").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_unknown_entry_type() {
        let line = r#"{"type":"progress"}"#;
        let mut parser = IncrementalParser::new();
        let events = parser.parse_line(line).unwrap();
        assert!(events.is_empty());
    }
}
