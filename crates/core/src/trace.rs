use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level session - the root of a HAIL (Human AI Interaction Log) trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Format version, e.g. "hail-1.0.0"
    pub version: String,
    /// Unique session identifier (UUID)
    pub session_id: String,
    /// AI agent information
    pub agent: Agent,
    /// Session metadata
    pub context: SessionContext,
    /// Flat timeline of events
    pub events: Vec<Event>,
    /// Aggregate statistics
    pub stats: Stats,
}

impl Session {
    pub const CURRENT_VERSION: &'static str = "hail-1.0.0";

    pub fn new(session_id: String, agent: Agent) -> Self {
        Self {
            version: Self::CURRENT_VERSION.to_string(),
            session_id,
            agent,
            context: SessionContext::default(),
            events: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// Recompute stats from events
    pub fn recompute_stats(&mut self) {
        let mut message_count = 0u64;
        let mut tool_call_count = 0u64;
        let mut task_ids = std::collections::HashSet::new();

        for event in &self.events {
            match &event.event_type {
                EventType::UserMessage | EventType::AgentMessage => message_count += 1,
                EventType::ToolCall { .. }
                | EventType::FileRead { .. }
                | EventType::CodeSearch { .. }
                | EventType::FileSearch { .. } => tool_call_count += 1,
                _ => {}
            }
            if let Some(ref tid) = event.task_id {
                task_ids.insert(tid.clone());
            }
        }

        let duration_seconds = if let (Some(first), Some(last)) =
            (self.events.first(), self.events.last())
        {
            (last.timestamp - first.timestamp).num_seconds().max(0) as u64
        } else {
            0
        };

        self.stats = Stats {
            event_count: self.events.len() as u64,
            message_count,
            tool_call_count,
            task_count: task_ids.len() as u64,
            duration_seconds,
        };
    }
}

/// AI agent information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// Provider: "anthropic", "openai", "local"
    pub provider: String,
    /// Model: "claude-opus-4-6", "gpt-4o"
    pub model: String,
    /// Tool: "claude-code", "cursor", "goose"
    pub tool: String,
    /// Tool version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
}

/// Session context metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

impl Default for SessionContext {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            title: None,
            description: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            attributes: HashMap::new(),
        }
    }
}

/// A single event in the flat timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier
    pub event_id: String,
    /// When this event occurred
    pub timestamp: DateTime<Utc>,
    /// Type of event
    pub event_type: EventType,
    /// Optional task grouping ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Multimodal content
    pub content: Content,
    /// Duration in milliseconds (for tool calls, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Arbitrary metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

/// Event type - the core abstraction
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventType {
    // Conversation
    UserMessage,
    AgentMessage,
    SystemMessage,

    // AI internals
    Thinking,

    // Tools/Actions
    ToolCall {
        name: String,
    },
    ToolResult {
        name: String,
        is_error: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    FileRead {
        path: String,
    },
    CodeSearch {
        query: String,
    },
    FileSearch {
        pattern: String,
    },
    FileEdit {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    FileCreate {
        path: String,
    },
    FileDelete {
        path: String,
    },
    ShellCommand {
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },

    // Multimodal generation
    ImageGenerate {
        prompt: String,
    },
    VideoGenerate {
        prompt: String,
    },
    AudioGenerate {
        prompt: String,
    },

    // Search/Reference
    WebSearch {
        query: String,
    },
    WebFetch {
        url: String,
    },

    // Task boundary markers (optional)
    TaskStart {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    TaskEnd {
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    // Extension point
    Custom {
        kind: String,
    },
}

/// Multimodal content container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub blocks: Vec<ContentBlock>,
}

impl Content {
    pub fn empty() -> Self {
        Self { blocks: Vec::new() }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            blocks: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn code(code: impl Into<String>, language: Option<String>) -> Self {
        Self {
            blocks: vec![ContentBlock::Code {
                code: code.into(),
                language,
                start_line: None,
            }],
        }
    }
}

/// Individual content block
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Code {
        code: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        start_line: Option<u32>,
    },
    Image {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
        mime: String,
    },
    Video {
        url: String,
        mime: String,
    },
    Audio {
        url: String,
        mime: String,
    },
    File {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    Json {
        data: serde_json::Value,
    },
    Reference {
        uri: String,
        media_type: String,
    },
}

/// Aggregate session statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub event_count: u64,
    pub message_count: u64,
    pub tool_call_count: u64,
    pub task_count: u64,
    pub duration_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_roundtrip() {
        let session = Session::new(
            "test-session-id".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: Some("1.0.0".to_string()),
            },
        );

        let json = serde_json::to_string_pretty(&session).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "hail-1.0.0");
        assert_eq!(parsed.session_id, "test-session-id");
        assert_eq!(parsed.agent.provider, "anthropic");
    }

    #[test]
    fn test_event_type_serialization() {
        let event_type = EventType::ToolCall {
            name: "Read".to_string(),
        };
        let json = serde_json::to_string(&event_type).unwrap();
        assert!(json.contains("ToolCall"));
        assert!(json.contains("Read"));

        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::ToolCall { name } => assert_eq!(name, "Read"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_content_block_variants() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Hello".to_string(),
            },
            ContentBlock::Code {
                code: "fn main() {}".to_string(),
                language: Some("rust".to_string()),
                start_line: None,
            },
            ContentBlock::Image {
                url: "https://example.com/img.png".to_string(),
                alt: Some("Screenshot".to_string()),
                mime: "image/png".to_string(),
            },
        ];

        let content = Content { blocks };
        let json = serde_json::to_string_pretty(&content).unwrap();
        let parsed: Content = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.blocks.len(), 3);
    }

    #[test]
    fn test_recompute_stats() {
        let mut session = Session::new(
            "test".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );

        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: Some("t1".to_string()),
            content: Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::ToolCall {
                name: "Read".to_string(),
            },
            task_id: Some("t1".to_string()),
            content: Content::empty(),
            duration_ms: Some(100),
            attributes: HashMap::new(),
        });

        session.events.push(Event {
            event_id: "e3".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::AgentMessage,
            task_id: Some("t2".to_string()),
            content: Content::text("done"),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        session.recompute_stats();
        assert_eq!(session.stats.event_count, 3);
        assert_eq!(session.stats.message_count, 2);
        assert_eq!(session.stats.tool_call_count, 1);
        assert_eq!(session.stats.task_count, 2);
    }

    #[test]
    fn test_file_read_serialization() {
        let et = EventType::FileRead {
            path: "/tmp/test.rs".to_string(),
        };
        let json = serde_json::to_string(&et).unwrap();
        assert!(json.contains("FileRead"));
        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::FileRead { path } => assert_eq!(path, "/tmp/test.rs"),
            _ => panic!("Expected FileRead"),
        }
    }

    #[test]
    fn test_code_search_serialization() {
        let et = EventType::CodeSearch {
            query: "fn main".to_string(),
        };
        let json = serde_json::to_string(&et).unwrap();
        assert!(json.contains("CodeSearch"));
        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::CodeSearch { query } => assert_eq!(query, "fn main"),
            _ => panic!("Expected CodeSearch"),
        }
    }

    #[test]
    fn test_file_search_serialization() {
        let et = EventType::FileSearch {
            pattern: "**/*.rs".to_string(),
        };
        let json = serde_json::to_string(&et).unwrap();
        assert!(json.contains("FileSearch"));
        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::FileSearch { pattern } => assert_eq!(pattern, "**/*.rs"),
            _ => panic!("Expected FileSearch"),
        }
    }

    #[test]
    fn test_tool_result_with_call_id() {
        let et = EventType::ToolResult {
            name: "Read".to_string(),
            is_error: false,
            call_id: Some("call-123".to_string()),
        };
        let json = serde_json::to_string(&et).unwrap();
        assert!(json.contains("call_id"));
        assert!(json.contains("call-123"));
        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::ToolResult {
                name,
                is_error,
                call_id,
            } => {
                assert_eq!(name, "Read");
                assert!(!is_error);
                assert_eq!(call_id, Some("call-123".to_string()));
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_tool_result_without_call_id() {
        let et = EventType::ToolResult {
            name: "Bash".to_string(),
            is_error: true,
            call_id: None,
        };
        let json = serde_json::to_string(&et).unwrap();
        assert!(!json.contains("call_id"));
        let parsed: EventType = serde_json::from_str(&json).unwrap();
        match parsed {
            EventType::ToolResult { call_id, .. } => assert_eq!(call_id, None),
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_recompute_stats_new_tool_types() {
        let mut session = Session::new(
            "test2".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );

        let ts = Utc::now();
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: ts,
            event_type: EventType::FileRead {
                path: "/tmp/a.rs".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e2".to_string(),
            timestamp: ts,
            event_type: EventType::CodeSearch {
                query: "fn main".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e3".to_string(),
            timestamp: ts,
            event_type: EventType::FileSearch {
                pattern: "*.rs".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.events.push(Event {
            event_id: "e4".to_string(),
            timestamp: ts,
            event_type: EventType::ToolCall {
                name: "Task".to_string(),
            },
            task_id: None,
            content: Content::empty(),
            duration_ms: None,
            attributes: HashMap::new(),
        });

        session.recompute_stats();
        assert_eq!(session.stats.tool_call_count, 4);
    }
}
