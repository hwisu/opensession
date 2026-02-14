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
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            return false;
        }
        path.to_str().is_some_and(|s| {
            s.contains(".claude/projects/")
                || s.contains(".claude/projects\\")
                || s.contains("/.claude/projects/")
                || s.contains("\\.claude\\projects\\")
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn can_parse_only_matches_claude_projects_jsonl() {
        let parser = ClaudeCodeParser;
        assert!(parser.can_parse(Path::new("/Users/test/.claude/projects/foo/session.jsonl")));
        assert!(!parser.can_parse(Path::new(
            "/Users/test/.codex/sessions/2026/02/14/rollout.jsonl"
        )));
    }
}
