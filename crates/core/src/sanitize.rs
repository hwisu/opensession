use crate::trace::{ContentBlock, Event, EventType, Session};
use regex::Regex;
use std::sync::LazyLock;

/// Configuration for sanitization
#[derive(Debug, Clone)]
pub struct SanitizeConfig {
    /// Strip absolute file paths (replace with relative)
    pub strip_paths: bool,
    /// Strip environment variable values
    pub strip_env_vars: bool,
    /// Patterns to exclude (glob-like)
    pub exclude_patterns: Vec<String>,
}

impl Default for SanitizeConfig {
    fn default() -> Self {
        Self {
            strip_paths: true,
            strip_env_vars: true,
            exclude_patterns: vec![
                "*.env".to_string(),
                "*secret*".to_string(),
                "*credential*".to_string(),
                "*password*".to_string(),
                "*token*".to_string(),
                "*api_key*".to_string(),
                "*apikey*".to_string(),
            ],
        }
    }
}

static HOME_DIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(/Users/[^/\s]+|/home/[^/\s]+|C:\\Users\\[^\\\s]+)").unwrap());

static ENV_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(api[_-]?key|token|secret|password|credential|auth)[=:]\s*\S+").unwrap()
});

/// Sanitize a session in-place
pub fn sanitize_session(session: &mut Session, config: &SanitizeConfig) {
    for event in &mut session.events {
        sanitize_event(event, config);
    }
}

/// Sanitize a single event
pub fn sanitize_event(event: &mut Event, config: &SanitizeConfig) {
    // Sanitize event type fields
    match &mut event.event_type {
        EventType::FileEdit { path, .. }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => {
            if config.strip_paths {
                *path = strip_home_dir(path);
            }
        }
        EventType::ShellCommand { command, .. } => {
            if config.strip_env_vars {
                *command = strip_env_vars(command);
            }
            if config.strip_paths {
                *command = strip_home_dir(command);
            }
        }
        _ => {}
    }

    // Sanitize content blocks
    for block in &mut event.content.blocks {
        sanitize_content_block(block, config);
    }
}

fn sanitize_content_block(block: &mut ContentBlock, config: &SanitizeConfig) {
    match block {
        ContentBlock::Text { text } => {
            if config.strip_paths {
                *text = strip_home_dir(text);
            }
            if config.strip_env_vars {
                *text = strip_env_vars(text);
            }
        }
        ContentBlock::Code { code, .. } => {
            if config.strip_paths {
                *code = strip_home_dir(code);
            }
            if config.strip_env_vars {
                *code = strip_env_vars(code);
            }
        }
        ContentBlock::File { path, content, .. } => {
            if config.strip_paths {
                *path = strip_home_dir(path);
            }
            if let Some(c) = content {
                if config.strip_env_vars {
                    *c = strip_env_vars(c);
                }
            }
        }
        _ => {}
    }
}

/// Replace home directory paths with ~
fn strip_home_dir(text: &str) -> String {
    HOME_DIR_RE.replace_all(text, "~").to_string()
}

/// Replace environment variable values with [REDACTED]
fn strip_env_vars(text: &str) -> String {
    ENV_VAR_RE
        .replace_all(text, "[REDACTED_CREDENTIAL]")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_strip_home_dir() {
        assert_eq!(strip_home_dir("/Users/john/projects/foo"), "~/projects/foo");
        assert_eq!(strip_home_dir("/home/john/projects/foo"), "~/projects/foo");
    }

    #[test]
    fn test_strip_env_vars() {
        assert_eq!(
            strip_env_vars("API_KEY=sk-1234567890"),
            "[REDACTED_CREDENTIAL]"
        );
        assert_eq!(strip_env_vars("token: abc123def"), "[REDACTED_CREDENTIAL]");
    }

    #[test]
    fn test_sanitize_event() {
        let config = SanitizeConfig::default();
        let mut event = Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::FileEdit {
                path: "/Users/john/projects/foo/src/main.rs".to_string(),
                diff: None,
            },
            task_id: None,
            content: Content::text("Editing /Users/john/projects/foo/src/main.rs"),
            duration_ms: None,
            attributes: HashMap::new(),
        };

        sanitize_event(&mut event, &config);

        match &event.event_type {
            EventType::FileEdit { path, .. } => {
                assert_eq!(path, "~/projects/foo/src/main.rs");
            }
            _ => panic!("wrong type"),
        }

        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("~/projects/foo/src/main.rs"));
                assert!(!text.contains("/Users/john"));
            }
            _ => panic!("wrong block type"),
        }
    }

    fn make_event(event_type: EventType, content: Content) -> Event {
        Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content,
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn test_sanitize_code_block() {
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::AgentMessage,
            Content {
                blocks: vec![ContentBlock::Code {
                    code: "let path = \"/Users/alice/project/main.rs\"; API_KEY=sk-abc123"
                        .to_string(),
                    language: Some("rust".to_string()),
                    start_line: None,
                }],
            },
        );
        sanitize_event(&mut event, &config);
        match &event.content.blocks[0] {
            ContentBlock::Code { code, .. } => {
                assert!(!code.contains("/Users/alice"));
                assert!(code.contains("~/project/main.rs"));
                assert!(!code.contains("sk-abc123"));
                assert!(code.contains("[REDACTED_CREDENTIAL]"));
            }
            _ => panic!("expected Code block"),
        }
    }

    #[test]
    fn test_sanitize_file_block() {
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::AgentMessage,
            Content {
                blocks: vec![ContentBlock::File {
                    path: "/home/bob/docs/readme.md".to_string(),
                    content: Some("secret=hunter2".to_string()),
                }],
            },
        );
        sanitize_event(&mut event, &config);
        match &event.content.blocks[0] {
            ContentBlock::File { path, content, .. } => {
                assert_eq!(path, "~/docs/readme.md");
                let c = content.as_deref().unwrap();
                assert!(!c.contains("hunter2"));
                assert!(c.contains("[REDACTED_CREDENTIAL]"));
            }
            _ => panic!("expected File block"),
        }
    }

    #[test]
    fn test_sanitize_shell_command() {
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::ShellCommand {
                command: "TOKEN=abc123 /Users/alice/bin/run".to_string(),
                exit_code: Some(0),
            },
            Content::text("output"),
        );
        sanitize_event(&mut event, &config);
        match &event.event_type {
            EventType::ShellCommand { command, .. } => {
                assert!(!command.contains("abc123"));
                assert!(command.contains("[REDACTED_CREDENTIAL]"));
                assert!(!command.contains("/Users/alice"));
                assert!(command.contains("~/bin/run"));
            }
            _ => panic!("expected ShellCommand"),
        }
    }

    #[test]
    fn test_sanitize_file_edit() {
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::FileEdit {
                path: "/home/dev/project/src/lib.rs".to_string(),
                diff: Some("+ some diff".to_string()),
            },
            Content::text("edited file"),
        );
        sanitize_event(&mut event, &config);
        match &event.event_type {
            EventType::FileEdit { path, .. } => {
                assert_eq!(path, "~/project/src/lib.rs");
            }
            _ => panic!("expected FileEdit"),
        }
    }

    #[test]
    fn test_sanitize_tool_call() {
        // ToolCall has only a name field; sanitization applies to content blocks
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::ToolCall {
                name: "Bash".to_string(),
            },
            Content::text("Running /Users/alice/scripts/deploy.sh with password=abc123"),
        );
        sanitize_event(&mut event, &config);
        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(!text.contains("/Users/alice"));
                assert!(text.contains("~/scripts/deploy.sh"));
                assert!(!text.contains("abc123"));
                assert!(text.contains("[REDACTED_CREDENTIAL]"));
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_config_strip_paths_false() {
        let config = SanitizeConfig {
            strip_paths: false,
            strip_env_vars: true,
            exclude_patterns: vec![],
        };
        let mut event = make_event(
            EventType::FileEdit {
                path: "/Users/john/project/main.rs".to_string(),
                diff: None,
            },
            Content::text("Editing /Users/john/project/main.rs"),
        );
        sanitize_event(&mut event, &config);
        match &event.event_type {
            EventType::FileEdit { path, .. } => {
                // Path should NOT be stripped
                assert_eq!(path, "/Users/john/project/main.rs");
            }
            _ => panic!("expected FileEdit"),
        }
        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("/Users/john/project/main.rs"));
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_config_exclude_patterns() {
        // exclude_patterns is a config field for downstream consumers; sanitize_event
        // itself does not filter events — it only transforms content in-place.
        // Verify the default patterns are populated correctly.
        let config = SanitizeConfig::default();
        assert!(config.exclude_patterns.contains(&"*.env".to_string()));
        assert!(config.exclude_patterns.contains(&"*secret*".to_string()));
        assert!(config.exclude_patterns.contains(&"*token*".to_string()));
        assert_eq!(config.exclude_patterns.len(), 7);
    }

    #[test]
    fn test_config_strip_env_false() {
        let config = SanitizeConfig {
            strip_paths: true,
            strip_env_vars: false,
            exclude_patterns: vec![],
        };
        let mut event = make_event(
            EventType::ShellCommand {
                command: "API_KEY=sk-12345 /Users/alice/bin/run".to_string(),
                exit_code: Some(0),
            },
            Content::text("token: abc123"),
        );
        sanitize_event(&mut event, &config);
        match &event.event_type {
            EventType::ShellCommand { command, .. } => {
                // Env vars should NOT be stripped
                assert!(command.contains("API_KEY=sk-12345"));
                // But paths should still be stripped
                assert!(!command.contains("/Users/alice"));
                assert!(command.contains("~/bin/run"));
            }
            _ => panic!("expected ShellCommand"),
        }
        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                // Env vars in content should NOT be stripped
                assert!(text.contains("token: abc123"));
            }
            _ => panic!("expected Text block"),
        }
    }
}
