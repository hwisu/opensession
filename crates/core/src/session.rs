use crate::trace::Session;

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

pub fn build_git_storage_meta_json(session: &Session) -> Vec<u8> {
    let role = match session_role(session) {
        SessionRole::Primary => "primary",
        SessionRole::Auxiliary => "auxiliary",
    };

    serde_json::to_vec_pretty(&serde_json::json!({
        "session_id": session.session_id,
        "title": session.context.title,
        "tool": session.agent.tool,
        "model": session.agent.model,
        "session_role": role,
        "stats": session.stats,
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        is_auxiliary_session, session_role, source_path, working_directory, SessionRole,
        ATTR_PARENT_SESSION_ID, ATTR_SESSION_ROLE,
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
}
