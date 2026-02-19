use crate::{extract::extract_file_metadata, EventType, Session};
use std::collections::HashMap;
use std::sync::Arc;

pub const DEFAULT_SCORE_PLUGIN: &str = "heuristic_v1";

/// A scoring plugin maps one session to one numeric score.
pub trait SessionScorePlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn score(&self, session: &Session) -> i64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionScore {
    pub plugin: String,
    pub score: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionScoreError {
    UnknownPlugin {
        requested: String,
        available: Vec<String>,
    },
}

impl std::fmt::Display for SessionScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPlugin {
                requested,
                available,
            } => {
                write!(
                    f,
                    "unknown session score plugin '{requested}'. available: {}",
                    available.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for SessionScoreError {}

/// Runtime registry for session score plugins.
pub struct SessionScoreRegistry {
    default_plugin: String,
    plugins: HashMap<String, Arc<dyn SessionScorePlugin>>,
}

impl Default for SessionScoreRegistry {
    fn default() -> Self {
        let mut registry = Self::new(DEFAULT_SCORE_PLUGIN);
        registry.register(HeuristicV1ScorePlugin);
        registry.register(ZeroV1ScorePlugin);
        registry
    }
}

impl SessionScoreRegistry {
    pub fn new(default_plugin: &str) -> Self {
        Self {
            default_plugin: default_plugin.to_string(),
            plugins: HashMap::new(),
        }
    }

    pub fn register<P>(&mut self, plugin: P)
    where
        P: SessionScorePlugin + 'static,
    {
        self.plugins
            .insert(plugin.id().to_string(), Arc::new(plugin));
    }

    pub fn available_plugins(&self) -> Vec<String> {
        let mut names: Vec<String> = self.plugins.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn score_default(&self, session: &Session) -> Result<SessionScore, SessionScoreError> {
        self.score_with(self.default_plugin.as_str(), session)
    }

    pub fn score_with(
        &self,
        plugin_id: &str,
        session: &Session,
    ) -> Result<SessionScore, SessionScoreError> {
        let plugin =
            self.plugins
                .get(plugin_id)
                .ok_or_else(|| SessionScoreError::UnknownPlugin {
                    requested: plugin_id.to_string(),
                    available: self.available_plugins(),
                })?;
        Ok(SessionScore {
            plugin: plugin_id.to_string(),
            score: plugin.score(session),
        })
    }
}

/// Default heuristic scorer.
///
/// Formula:
/// - Base 100
/// - `has_errors` => -15
/// - shell failures (`exit_code != 0`) => -5 each (cap -30)
/// - tool errors (`ToolResult.is_error=true`) => -4 each (cap -20)
/// - recovery (same task lane: failure -> success) => +5 each (cap +20)
/// - clamp to 0..100
pub struct HeuristicV1ScorePlugin;

impl SessionScorePlugin for HeuristicV1ScorePlugin {
    fn id(&self) -> &'static str {
        "heuristic_v1"
    }

    fn score(&self, session: &Session) -> i64 {
        let (_, _, has_errors) = extract_file_metadata(session);
        let shell_failures = count_shell_failures(session) as i64;
        let tool_errors = count_tool_errors(session) as i64;
        let recoveries = count_recoveries(session) as i64;

        let mut score = 100i64;
        if has_errors {
            score -= 15;
        }
        score -= (shell_failures * 5).min(30);
        score -= (tool_errors * 4).min(20);
        score += (recoveries * 5).min(20);
        score.clamp(0, 100)
    }
}

/// A deterministic scorer useful for testing and compatibility checks.
pub struct ZeroV1ScorePlugin;

impl SessionScorePlugin for ZeroV1ScorePlugin {
    fn id(&self) -> &'static str {
        "zero_v1"
    }

    fn score(&self, _session: &Session) -> i64 {
        0
    }
}

fn count_shell_failures(session: &Session) -> usize {
    session
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.event_type,
                EventType::ShellCommand {
                    exit_code: Some(code),
                    ..
                } if *code != 0
            )
        })
        .count()
}

fn count_tool_errors(session: &Session) -> usize {
    session
        .events
        .iter()
        .filter(|event| {
            matches!(
                &event.event_type,
                EventType::ToolResult { is_error: true, .. }
            )
        })
        .count()
}

fn event_task_key(task_id: &Option<String>) -> String {
    task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("__global__")
        .to_string()
}

fn count_recoveries(session: &Session) -> usize {
    let mut pending_failures: HashMap<String, usize> = HashMap::new();
    let mut recoveries = 0usize;

    for event in &session.events {
        let key = event_task_key(&event.task_id);
        match &event.event_type {
            EventType::ShellCommand {
                exit_code: Some(code),
                ..
            } if *code != 0 => {
                *pending_failures.entry(key).or_default() += 1;
            }
            EventType::ToolResult { is_error: true, .. } => {
                *pending_failures.entry(key).or_default() += 1;
            }
            EventType::ShellCommand {
                exit_code: Some(0), ..
            }
            | EventType::ToolResult {
                is_error: false, ..
            } => {
                let mut remove = false;
                if let Some(pending) = pending_failures.get_mut(&key) {
                    if *pending > 0 {
                        *pending -= 1;
                        recoveries += 1;
                    }
                    if *pending == 0 {
                        remove = true;
                    }
                }
                if remove {
                    pending_failures.remove(&key);
                }
            }
            _ => {}
        }
    }

    recoveries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testing, Session};

    fn build_session(events: Vec<crate::Event>) -> Session {
        let mut session = Session::new("score-test".to_string(), testing::agent());
        session.events = events;
        session.recompute_stats();
        session
    }

    #[test]
    fn registry_contains_builtin_plugins() {
        let registry = SessionScoreRegistry::default();
        let names = registry.available_plugins();
        assert!(names.contains(&"heuristic_v1".to_string()));
        assert!(names.contains(&"zero_v1".to_string()));
    }

    #[test]
    fn heuristic_v1_penalizes_failures_and_rewards_recovery() {
        let mut fail = testing::event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(101),
            },
            "",
        );
        fail.task_id = Some("t1".to_string());

        let mut success = testing::event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(0),
            },
            "",
        );
        success.task_id = Some("t1".to_string());

        let session = build_session(vec![fail, success]);
        let registry = SessionScoreRegistry::default();
        let result = registry
            .score_with("heuristic_v1", &session)
            .expect("heuristic scorer must exist");

        // 100 -15(has_errors) -5(shell fail) +5(recovery)
        assert_eq!(result.score, 85);
    }

    #[test]
    fn zero_plugin_returns_zero() {
        let session = build_session(vec![testing::event(EventType::UserMessage, "hello")]);
        let registry = SessionScoreRegistry::default();
        let result = registry
            .score_with("zero_v1", &session)
            .expect("zero scorer must exist");
        assert_eq!(result.score, 0);
    }

    #[test]
    fn unknown_plugin_reports_available_names() {
        let session = build_session(vec![]);
        let registry = SessionScoreRegistry::default();
        let err = registry
            .score_with("missing_plugin", &session)
            .expect_err("must fail for unknown plugin");

        match err {
            SessionScoreError::UnknownPlugin {
                requested,
                available,
            } => {
                assert_eq!(requested, "missing_plugin");
                assert!(available.contains(&"heuristic_v1".to_string()));
            }
        }
    }
}
