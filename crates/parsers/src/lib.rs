pub mod claude_code;
pub mod incremental;

mod amp;
mod cline;
mod codex;
pub(crate) mod common;
mod cursor;
pub mod external;
mod gemini;
mod ingest;
mod opencode;

use anyhow::Result;
use opensession_core::trace::Session;
use std::path::Path;

pub use ingest::{ParseCandidate, ParseError, ParsePreview};

/// Trait for parsing AI tool session data into HAIL format
pub trait SessionParser: Send + Sync {
    /// Parser name (e.g. "claude-code", "codex")
    fn name(&self) -> &str;

    /// Check if this parser can handle the given path
    fn can_parse(&self, path: &std::path::Path) -> bool;

    /// Parse a session file/directory into HAIL format
    fn parse(&self, path: &std::path::Path) -> Result<Session>;
}

pub struct ParserRegistry {
    parsers: Vec<Box<dyn SessionParser>>,
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self {
            parsers: vec![
                Box::new(codex::CodexParser),
                Box::new(opencode::OpenCodeParser),
                Box::new(cline::ClineParser),
                Box::new(amp::AmpParser),
                Box::new(cursor::CursorParser),
                Box::new(gemini::GeminiParser),
                Box::new(claude_code::ClaudeCodeParser),
            ],
        }
    }
}

impl ParserRegistry {
    pub fn parser_for_path(&self, path: &Path) -> Option<&dyn SessionParser> {
        self.parsers
            .iter()
            .find(|parser| parser.can_parse(path))
            .map(|parser| parser.as_ref())
    }

    pub fn parse_path(&self, path: &Path) -> Result<Option<Session>> {
        let Some(parser) = self.parser_for_path(path) else {
            return Ok(None);
        };
        let session = parser.parse(path)?;
        Ok(Some(session))
    }

    pub fn preview_bytes(
        &self,
        filename: &str,
        content_bytes: &[u8],
        parser_hint: Option<&str>,
    ) -> Result<ParsePreview, ParseError> {
        ingest::preview_parse_bytes(filename, content_bytes, parser_hint)
    }
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

    #[test]
    fn parser_registry_exposes_default_parsers() {
        let registry = ParserRegistry::default();
        assert!(
            registry
                .parser_for_path(Path::new("/tmp/.codex/sessions/session.jsonl"))
                .is_some()
        );
        assert!(
            registry
                .parser_for_path(Path::new("/tmp/.claude/projects/demo/session.jsonl"))
                .is_some()
        );
    }

    #[test]
    fn parser_registry_parses_known_fixture_paths() {
        let registry = ParserRegistry::default();
        let fixture_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex/rollout-desktop.jsonl");
        let fixture_dir = std::env::temp_dir().join(format!(
            "opensession-parser-registry-{}",
            std::process::id()
        ));
        let fixture = fixture_dir.join(".codex/sessions/rollout-desktop.jsonl");
        std::fs::create_dir_all(
            fixture
                .parent()
                .expect("fixture path should include a parent directory"),
        )
        .expect("temp codex fixture directory should exist");
        std::fs::copy(&fixture_src, &fixture).expect("fixture should copy to codex temp path");

        let parser = registry
            .parser_for_path(&fixture)
            .expect("fixture should resolve to a parser");
        assert_eq!(parser.name(), "codex");

        let session = registry
            .parse_path(&fixture)
            .expect("fixture parse should succeed")
            .expect("fixture should produce a session");
        assert_eq!(session.agent.tool, "codex");

        std::fs::remove_dir_all(fixture_dir).expect("temp fixture directory should be removable");
    }
}
