mod parse;
mod transform;

use crate::SessionParser;
use anyhow::Result;
use opensession_core::trace::{Agent, Event, Session, SessionContext};
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
        parse::parse_claude_code_jsonl(path)
    }
}

impl ClaudeCodeParser {
    /// Parse raw JSONL lines into HAIL components (agent, context, events).
    ///
    /// Used by `stream-push` to incrementally parse new lines without reading
    /// the full file. Returns the session ID found in the lines, if any.
    pub fn parse_lines(
        lines: &[String],
    ) -> (
        Option<Agent>,
        Option<SessionContext>,
        Vec<Event>,
        Option<String>,
    ) {
        let parsed = parse::parse_lines_impl(lines);
        (
            parsed.agent,
            parsed.context,
            parsed.events,
            parsed.session_id,
        )
    }
}

// Re-export pub(crate) items needed by incremental.rs
pub(crate) use parse::{
    parse_timestamp, process_assistant_entry, process_user_entry, RawConversationEntry, RawEntry,
};
