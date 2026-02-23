use crate::trace::Session;
use serde::{Deserialize, Serialize};

pub const ATTR_CWD: &str = "cwd";
pub const ATTR_WORKING_DIRECTORY: &str = "working_directory";
pub const ATTR_SOURCE_PATH: &str = "source_path";
pub const ATTR_SESSION_ROLE: &str = "session_role";
pub const ATTR_PARENT_SESSION_ID: &str = "parent_session_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRole {
    Primary,
    Auxiliary,
}

fn attr_non_empty_str<'a>(session: &'a Session, key: &str) -> Option<&'a str> {
    session
        .context
        .attributes
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn working_directory(session: &Session) -> Option<&str> {
    attr_non_empty_str(session, ATTR_CWD)
        .or_else(|| attr_non_empty_str(session, ATTR_WORKING_DIRECTORY))
}

pub fn source_path(session: &Session) -> Option<&str> {
    attr_non_empty_str(session, ATTR_SOURCE_PATH)
}

pub fn session_role(session: &Session) -> SessionRole {
    if let Some(raw_role) = attr_non_empty_str(session, ATTR_SESSION_ROLE) {
        if raw_role.eq_ignore_ascii_case("auxiliary") {
            return SessionRole::Auxiliary;
        }
        if raw_role.eq_ignore_ascii_case("primary") {
            return SessionRole::Primary;
        }
    }

    if !session.context.related_session_ids.is_empty() {
        return SessionRole::Auxiliary;
    }

    if attr_non_empty_str(session, ATTR_PARENT_SESSION_ID).is_some() {
        return SessionRole::Auxiliary;
    }

    SessionRole::Primary
}

pub fn is_auxiliary_session(session: &Session) -> bool {
    session_role(session) == SessionRole::Auxiliary
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
}

pub fn build_git_storage_meta_json_with_git(session: &Session, git: Option<&GitMeta>) -> Vec<u8> {
    let role = match session_role(session) {
        SessionRole::Primary => "primary",
        SessionRole::Auxiliary => "auxiliary",
    };

    let mut payload = serde_json::json!({
        "schema_version": 2,
        "session_id": session.session_id,
        "title": session.context.title,
        "tool": session.agent.tool,
        "model": session.agent.model,
        "session_role": role,
        "stats": session.stats,
    });

    if let Some(git_meta) = git {
        let has_git = git_meta.remote.is_some()
            || git_meta.repo_name.is_some()
            || git_meta.branch.is_some()
            || git_meta.head.is_some()
            || !git_meta.commits.is_empty();
        if has_git {
            payload["git"] = serde_json::to_value(git_meta).unwrap_or_default();
        }
    }

    serde_json::to_vec_pretty(&payload).unwrap_or_default()
}

pub fn build_git_storage_meta_json(session: &Session) -> Vec<u8> {
    build_git_storage_meta_json_with_git(session, None)
}

#[cfg(test)]
mod tests {
    use super::{
        build_git_storage_meta_json, build_git_storage_meta_json_with_git, is_auxiliary_session,
        session_role, source_path, working_directory, GitMeta, SessionRole, ATTR_PARENT_SESSION_ID,
        ATTR_SESSION_ROLE,
    };
    use crate::trace::{Agent, Session};
    use serde_json::Value;

    fn make_session() -> Session {
        Session::new(
            "s1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        )
    }

    #[test]
    fn working_directory_prefers_cwd() {
        let mut session = make_session();
        session.context.attributes.insert(
            "cwd".to_string(),
            Value::String("/repo/preferred".to_string()),
        );
        session.context.attributes.insert(
            "working_directory".to_string(),
            Value::String("/repo/fallback".to_string()),
        );

        assert_eq!(working_directory(&session), Some("/repo/preferred"));
    }

    #[test]
    fn working_directory_uses_working_directory_fallback() {
        let mut session = make_session();
        session.context.attributes.insert(
            "working_directory".to_string(),
            Value::String("/repo/fallback".to_string()),
        );

        assert_eq!(working_directory(&session), Some("/repo/fallback"));
    }

    #[test]
    fn source_path_returns_non_empty_value() {
        let mut session = make_session();
        session.context.attributes.insert(
            "source_path".to_string(),
            Value::String("/tmp/session.jsonl".to_string()),
        );

        assert_eq!(source_path(&session), Some("/tmp/session.jsonl"));
    }

    #[test]
    fn session_role_uses_explicit_attribute_first() {
        let mut session = make_session();
        session.context.related_session_ids = vec!["parent-id".to_string()];
        session.context.attributes.insert(
            ATTR_SESSION_ROLE.to_string(),
            Value::String("primary".to_string()),
        );

        assert_eq!(session_role(&session), SessionRole::Primary);
        assert!(!is_auxiliary_session(&session));
    }

    #[test]
    fn session_role_uses_related_session_ids() {
        let mut session = make_session();
        session.context.related_session_ids = vec!["parent-id".to_string()];

        assert_eq!(session_role(&session), SessionRole::Auxiliary);
        assert!(is_auxiliary_session(&session));
    }

    #[test]
    fn session_role_uses_parent_session_id_attribute() {
        let mut session = make_session();
        session.context.attributes.insert(
            ATTR_PARENT_SESSION_ID.to_string(),
            Value::String("parent-id".to_string()),
        );

        assert_eq!(session_role(&session), SessionRole::Auxiliary);
    }

    #[test]
    fn session_role_defaults_to_primary() {
        let session = make_session();
        assert_eq!(session_role(&session), SessionRole::Primary);
    }

    #[test]
    fn git_storage_meta_defaults_to_schema_v2_without_git() {
        let session = make_session();
        let bytes = build_git_storage_meta_json(&session);
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        assert_eq!(parsed["schema_version"], 2);
        assert!(parsed.get("git").is_none());
    }

    #[test]
    fn git_storage_meta_includes_git_block_when_present() {
        let session = make_session();
        let git = GitMeta {
            remote: Some("git@github.com:org/repo.git".to_string()),
            repo_name: Some("org/repo".to_string()),
            branch: Some("feature/x".to_string()),
            head: Some("abcd1234".to_string()),
            commits: vec!["abcd1234".to_string(), "beef5678".to_string()],
        };
        let bytes = build_git_storage_meta_json_with_git(&session, Some(&git));
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
        assert_eq!(parsed["schema_version"], 2);
        assert_eq!(parsed["git"]["repo_name"], "org/repo");
        assert_eq!(parsed["git"]["commits"][0], "abcd1234");
    }
}
