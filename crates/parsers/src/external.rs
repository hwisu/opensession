use crate::SessionParser;
use anyhow::{Context, Result};
use opensession_core::trace::Session;
use std::path::Path;
use std::process::Command;

/// Configuration for a custom external parser
#[derive(Debug, Clone)]
pub struct ExternalParserConfig {
    pub name: String,
    pub command: String,
    pub glob: String,
}

/// An external parser that delegates to an external command.
/// Protocol: command receives file path as argument, outputs HAIL Session JSON to stdout.
pub struct ExternalParser {
    pub config: ExternalParserConfig,
}

impl ExternalParser {
    pub fn new(config: ExternalParserConfig) -> Self {
        Self { config }
    }
}

impl SessionParser for ExternalParser {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn can_parse(&self, path: &Path) -> bool {
        // Use glob pattern matching
        let pattern = shellexpand::tilde(&self.config.glob).to_string();
        if let Ok(glob_pattern) = glob::Pattern::new(&pattern) {
            glob_pattern.matches_path(path)
        } else {
            false
        }
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        let output = Command::new(&self.config.command)
            .arg(path)
            .output()
            .with_context(|| {
                format!(
                    "Failed to run external parser '{}': {}",
                    self.config.name, self.config.command
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "External parser '{}' failed (exit {}): {}",
                self.config.name,
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }

        let session: Session = serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "External parser '{}' returned invalid HAIL JSON",
                self.config.name
            )
        })?;

        // Validate version
        if !session.version.starts_with("hail-") {
            anyhow::bail!(
                "External parser '{}' returned invalid version: {}",
                self.config.name,
                session.version
            );
        }

        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_external_parser_config() {
        let config = ExternalParserConfig {
            name: "test-tool".to_string(),
            command: "/usr/bin/echo".to_string(),
            glob: "~/.test-tool/**/*.json".to_string(),
        };
        let parser = ExternalParser::new(config);
        assert_eq!(parser.name(), "test-tool");
    }

    #[test]
    fn test_can_parse_glob() {
        let config = ExternalParserConfig {
            name: "test".to_string(),
            command: "echo".to_string(),
            glob: "/tmp/sessions/*.json".to_string(),
        };
        let parser = ExternalParser::new(config);
        assert!(parser.can_parse(Path::new("/tmp/sessions/test.json")));
        assert!(!parser.can_parse(Path::new("/other/test.json")));
    }
}
