pub mod claude_code;
pub mod discover;
pub mod incremental;
pub mod ingest;

mod amp;
mod cline;
mod codex;
pub(crate) mod common;
mod cursor;
pub mod external;
mod gemini;
mod opencode;

use anyhow::Result;
use opensession_core::trace::Session;
use std::path::Path;

/// Trait for parsing AI tool session data into HAIL format
pub trait SessionParser: Send + Sync {
    /// Parser name (e.g. "claude-code", "codex")
    fn name(&self) -> &str;

    /// Check if this parser can handle the given path
    fn can_parse(&self, path: &std::path::Path) -> bool;

    /// Parse a session file/directory into HAIL format
    fn parse(&self, path: &std::path::Path) -> Result<Session>;
}

/// Get all available parsers
pub fn all_parsers() -> Vec<Box<dyn SessionParser>> {
    vec![
        Box::new(codex::CodexParser),
        Box::new(opencode::OpenCodeParser),
        Box::new(cline::ClineParser),
        Box::new(amp::AmpParser),
        Box::new(cursor::CursorParser),
        Box::new(gemini::GeminiParser),
        Box::new(claude_code::ClaudeCodeParser),
    ]
}

/// Returns true when the path points to an auxiliary child/sub-agent session log.
///
/// This keeps caller crates (`cli`, `tui`) decoupled from parser-specific modules.
pub fn is_auxiliary_session_path(path: &Path) -> bool {
    claude_code::is_claude_subagent_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn auxiliary_path_detects_claude_subagent_logs() {
        assert!(is_auxiliary_session_path(Path::new(
            "/Users/test/.claude/projects/foo/subagents/agent-123.jsonl"
        )));
        assert!(!is_auxiliary_session_path(Path::new(
            "/Users/test/.claude/projects/foo/session.jsonl"
        )));
    }
}
