pub mod claude_code;
pub mod discover;
pub mod incremental;

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
        Box::new(claude_code::ClaudeCodeParser),
        Box::new(codex::CodexParser),
        Box::new(opencode::OpenCodeParser),
        Box::new(cline::ClineParser),
        Box::new(amp::AmpParser),
        Box::new(cursor::CursorParser),
        Box::new(gemini::GeminiParser),
    ]
}
