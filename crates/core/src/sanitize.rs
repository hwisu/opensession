use crate::trace::{ContentBlock, Event, EventType, Session};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const REDACTED_CREDENTIAL: &str = "[REDACTED_CREDENTIAL]";
const REDACTED_SENSITIVE_FILE: &str = "[REDACTED_SENSITIVE_FILE]";
const REDACTED_SENSITIVE_PATH: &str = "[REDACTED_SENSITIVE_PATH]";

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
                "*.env.*".to_string(),
                "*.zshrc".to_string(),
                "*.zprofile".to_string(),
                "*.zlogin".to_string(),
                "*.bashrc".to_string(),
                "*.bash_profile".to_string(),
                "*.profile".to_string(),
                "*.npmrc".to_string(),
                "*.pypirc".to_string(),
                "*.netrc".to_string(),
                "*.git-credentials".to_string(),
                ".ssh/*".to_string(),
                "*/.ssh/*".to_string(),
                "*.pem".to_string(),
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

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S+").unwrap());

/// Sanitize a session in-place
pub fn sanitize_session(session: &mut Session, config: &SanitizeConfig) {
    sanitize_session_context(session, config);
    for event in &mut session.events {
        sanitize_event(event, config);
    }
}

fn sanitize_session_context(session: &mut Session, config: &SanitizeConfig) {
    if let Some(title) = session.context.title.as_mut() {
        *title = sanitize_free_text(title, config);
    }
    if let Some(description) = session.context.description.as_mut() {
        *description = sanitize_free_text(description, config);
    }
    for tag in &mut session.context.tags {
        *tag = sanitize_free_text(tag, config);
    }
    for value in session.context.attributes.values_mut() {
        sanitize_json_value(value, config);
    }
}

/// Sanitize a single event
pub fn sanitize_event(event: &mut Event, config: &SanitizeConfig) {
    for value in event.attributes.values_mut() {
        sanitize_json_value(value, config);
    }

    match &mut event.event_type {
        EventType::FileEdit { path, diff } => {
            let is_sensitive = is_excluded_path(path, &config.exclude_patterns);
            *path = sanitize_path_value(path, config);
            if is_sensitive {
                *diff = Some(REDACTED_SENSITIVE_FILE.to_string());
            } else if let Some(diff) = diff {
                *diff = sanitize_free_text(diff, config);
            }
        }
        EventType::FileRead { path }
        | EventType::FileCreate { path }
        | EventType::FileDelete { path } => {
            *path = sanitize_path_value(path, config);
        }
        EventType::ShellCommand { command, .. } => {
            *command = sanitize_free_text(command, config);
        }
        _ => {}
    }

    for block in &mut event.content.blocks {
        sanitize_content_block(block, config);
    }
}

fn sanitize_content_block(block: &mut ContentBlock, config: &SanitizeConfig) {
    match block {
        ContentBlock::Text { text } => {
            *text = sanitize_free_text(text, config);
        }
        ContentBlock::Code { code, .. } => {
            *code = sanitize_free_text(code, config);
        }
        ContentBlock::File { path, content, .. } => {
            let is_sensitive = is_excluded_path(path, &config.exclude_patterns);
            *path = sanitize_path_value(path, config);
            if is_sensitive {
                *content = Some(REDACTED_SENSITIVE_FILE.to_string());
            } else if let Some(content) = content {
                *content = sanitize_free_text(content, config);
            }
        }
        ContentBlock::Json { data } => sanitize_json_value(data, config),
        ContentBlock::Reference { uri, .. } => {
            *uri = sanitize_free_text(uri, config);
        }
        _ => {}
    }
}

fn sanitize_json_value(value: &mut Value, config: &SanitizeConfig) {
    match value {
        Value::String(text) => {
            *text = sanitize_free_text(text, config);
        }
        Value::Array(values) => {
            for value in values {
                sanitize_json_value(value, config);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                sanitize_json_value(value, config);
            }
        }
        _ => {}
    }
}

fn sanitize_path_value(path: &str, config: &SanitizeConfig) -> String {
    if is_excluded_path(path, &config.exclude_patterns) {
        return REDACTED_SENSITIVE_PATH.to_string();
    }

    if config.strip_paths {
        return strip_home_dir(path);
    }

    path.to_string()
}

fn sanitize_free_text(text: &str, config: &SanitizeConfig) -> String {
    let mut sanitized = text.to_string();
    if config.strip_paths {
        sanitized = strip_home_dir(&sanitized);
    }
    sanitized = redact_sensitive_path_tokens(&sanitized, &config.exclude_patterns);
    if config.strip_env_vars {
        sanitized = strip_env_vars(&sanitized);
    }
    sanitized
}

fn redact_sensitive_path_tokens(text: &str, patterns: &[String]) -> String {
    TOKEN_RE
        .replace_all(text, |caps: &regex::Captures<'_>| {
            let token = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
            redact_token_if_sensitive_path(token, patterns)
        })
        .to_string()
}

fn redact_token_if_sensitive_path(token: &str, patterns: &[String]) -> String {
    let trimmed = token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    if trimmed.is_empty() || !looks_like_path_token(trimmed) || !is_excluded_path(trimmed, patterns)
    {
        return token.to_string();
    }

    let prefix_len = token.find(trimmed).unwrap_or(0);
    let suffix_start = prefix_len + trimmed.len();
    let prefix = &token[..prefix_len];
    let suffix = &token[suffix_start..];
    format!("{prefix}{REDACTED_SENSITIVE_PATH}{suffix}")
}

fn looks_like_path_token(token: &str) -> bool {
    token.starts_with('~')
        || token.starts_with('.')
        || token.contains('/')
        || token.contains('\\')
        || token.contains('.')
}

fn is_excluded_path(path: &str, patterns: &[String]) -> bool {
    let normalized = path.trim().replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    let without_relative = normalized.strip_prefix("./").unwrap_or(normalized.as_str());
    let without_home = without_relative
        .strip_prefix("~/")
        .unwrap_or(without_relative);
    let basename = without_home.rsplit('/').next().unwrap_or(without_home);

    patterns.iter().any(|pattern| {
        let normalized_pattern = pattern.trim().replace('\\', "/").to_ascii_lowercase();
        !normalized_pattern.is_empty()
            && (wildcard_match(&normalized_pattern, &normalized)
                || wildcard_match(&normalized_pattern, without_relative)
                || wildcard_match(&normalized_pattern, without_home)
                || wildcard_match(&normalized_pattern, basename))
    })
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.as_bytes();
    let candidate = candidate.as_bytes();
    let mut pattern_idx = 0usize;
    let mut candidate_idx = 0usize;
    let mut star_idx: Option<usize> = None;
    let mut star_candidate_idx = 0usize;

    while candidate_idx < candidate.len() {
        if pattern_idx < pattern.len() && pattern[pattern_idx] == candidate[candidate_idx] {
            pattern_idx += 1;
            candidate_idx += 1;
            continue;
        }
        if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            pattern_idx += 1;
            star_candidate_idx = candidate_idx;
            continue;
        }
        if let Some(star) = star_idx {
            pattern_idx = star + 1;
            star_candidate_idx += 1;
            candidate_idx = star_candidate_idx;
            continue;
        }
        return false;
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }
    pattern_idx == pattern.len()
}

/// Replace home directory paths with ~
fn strip_home_dir(text: &str) -> String {
    HOME_DIR_RE.replace_all(text, "~").to_string()
}

/// Replace environment variable values with [REDACTED]
fn strip_env_vars(text: &str) -> String {
    ENV_VAR_RE
        .replace_all(text, REDACTED_CREDENTIAL)
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
        assert_eq!(strip_env_vars("API_KEY=sk-1234567890"), REDACTED_CREDENTIAL);
        assert_eq!(strip_env_vars("token: abc123def"), REDACTED_CREDENTIAL);
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
                assert!(code.contains(REDACTED_CREDENTIAL));
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
                let content = content.as_deref().unwrap();
                assert!(!content.contains("hunter2"));
                assert!(content.contains(REDACTED_CREDENTIAL));
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
                assert!(command.contains(REDACTED_CREDENTIAL));
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
            EventType::FileEdit { path, diff } => {
                assert_eq!(path, "~/project/src/lib.rs");
                assert_eq!(diff.as_deref(), Some("+ some diff"));
            }
            _ => panic!("expected FileEdit"),
        }
    }

    #[test]
    fn test_sanitize_tool_call() {
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
                assert!(text.contains(REDACTED_CREDENTIAL));
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
        let config = SanitizeConfig::default();
        assert!(config.exclude_patterns.contains(&"*.env".to_string()));
        assert!(config.exclude_patterns.contains(&"*.zshrc".to_string()));
        assert!(config.exclude_patterns.contains(&".ssh/*".to_string()));
        assert!(config.exclude_patterns.contains(&"*secret*".to_string()));
        assert!(config.exclude_patterns.contains(&"*token*".to_string()));
        assert!(config.exclude_patterns.len() >= 10);
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
                assert!(command.contains("API_KEY=sk-12345"));
                assert!(!command.contains("/Users/alice"));
                assert!(command.contains("~/bin/run"));
            }
            _ => panic!("expected ShellCommand"),
        }
        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(text.contains("token: abc123"));
            }
            _ => panic!("expected Text block"),
        }
    }

    #[test]
    fn test_sensitive_file_paths_are_redacted_everywhere() {
        let config = SanitizeConfig::default();
        let mut event = make_event(
            EventType::FileEdit {
                path: "/Users/alice/.zshrc".to_string(),
                diff: Some("+ export API_KEY=sk-secret".to_string()),
            },
            Content {
                blocks: vec![
                    ContentBlock::Text {
                        text: "cat /Users/alice/.zshrc".to_string(),
                    },
                    ContentBlock::File {
                        path: "/Users/alice/.zshrc".to_string(),
                        content: Some("export API_KEY=sk-secret".to_string()),
                    },
                ],
            },
        );

        sanitize_event(&mut event, &config);

        match &event.event_type {
            EventType::FileEdit { path, diff } => {
                assert_eq!(path, REDACTED_SENSITIVE_PATH);
                assert_eq!(diff.as_deref(), Some(REDACTED_SENSITIVE_FILE));
            }
            _ => panic!("expected FileEdit"),
        }
        match &event.content.blocks[0] {
            ContentBlock::Text { text } => {
                assert!(!text.contains(".zshrc"));
                assert!(text.contains(REDACTED_SENSITIVE_PATH));
            }
            _ => panic!("expected Text block"),
        }
        match &event.content.blocks[1] {
            ContentBlock::File { path, content, .. } => {
                assert_eq!(path, REDACTED_SENSITIVE_PATH);
                assert_eq!(content.as_deref(), Some(REDACTED_SENSITIVE_FILE));
            }
            _ => panic!("expected File block"),
        }
    }

    #[test]
    fn test_sanitize_session_context_metadata() {
        let config = SanitizeConfig::default();
        let mut session = Session::new(
            "s-meta".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.context.title = Some("Review /Users/alice/.zshrc".to_string());
        session.context.description = Some("API_KEY=sk-secret".to_string());
        session.context.attributes.insert(
            "cwd".to_string(),
            Value::String("/Users/alice/projects/repo".to_string()),
        );
        session.context.attributes.insert(
            "all_cwds".to_string(),
            Value::Array(vec![
                Value::String("/Users/alice/projects/repo".to_string()),
                Value::String("/Users/alice/.ssh/id_rsa".to_string()),
            ]),
        );

        sanitize_session(&mut session, &config);

        assert!(
            !session
                .context
                .title
                .as_deref()
                .unwrap_or_default()
                .contains(".zshrc")
        );
        assert!(
            session
                .context
                .title
                .as_deref()
                .unwrap_or_default()
                .contains(REDACTED_SENSITIVE_PATH)
        );
        assert_eq!(
            session.context.description.as_deref(),
            Some(REDACTED_CREDENTIAL)
        );
        assert_eq!(
            session
                .context
                .attributes
                .get("cwd")
                .and_then(Value::as_str),
            Some("~/projects/repo")
        );
        let all_cwds = session
            .context
            .attributes
            .get("all_cwds")
            .and_then(Value::as_array)
            .expect("all_cwds array");
        assert_eq!(all_cwds[0].as_str(), Some("~/projects/repo"));
        assert_eq!(all_cwds[1].as_str(), Some(REDACTED_SENSITIVE_PATH));
    }
}
