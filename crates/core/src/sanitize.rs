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

static HOME_DIR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(/Users/[^/\s]+|/home/[^/\s]+|C:\\Users\\[^\\\s]+)").unwrap()
});

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
fn sanitize_event(event: &mut Event, config: &SanitizeConfig) {
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
        assert_eq!(
            strip_home_dir("/Users/john/projects/foo"),
            "~/projects/foo"
        );
        assert_eq!(
            strip_home_dir("/home/john/projects/foo"),
            "~/projects/foo"
        );
    }

    #[test]
    fn test_strip_env_vars() {
        assert_eq!(
            strip_env_vars("API_KEY=sk-1234567890"),
            "[REDACTED_CREDENTIAL]"
        );
        assert_eq!(
            strip_env_vars("token: abc123def"),
            "[REDACTED_CREDENTIAL]"
        );
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
}
