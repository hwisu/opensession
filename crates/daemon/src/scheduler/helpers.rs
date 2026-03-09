use crate::config::DaemonConfig;
use opensession_core::Session;
use opensession_core::sanitize::{SanitizeConfig, sanitize_session};
use opensession_core::session::{GitMeta, build_git_storage_meta_json_with_git, working_directory};

pub(super) fn session_cwd(session: &Session) -> Option<&str> {
    working_directory(session)
}

pub(super) fn build_session_meta_json(session: &Session, git: Option<&GitMeta>) -> Vec<u8> {
    build_git_storage_meta_json_with_git(session, git)
}

pub(super) fn session_to_hail_jsonl_bytes(session: &Session) -> Option<Vec<u8>> {
    match session.to_jsonl() {
        Ok(jsonl) => Some(jsonl.into_bytes()),
        Err(error) => {
            tracing::warn!(
                "Failed to serialize session {} to HAIL JSONL: {}",
                session.session_id,
                error
            );
            None
        }
    }
}

pub(super) fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn sanitize(session: &mut Session, config: &DaemonConfig) {
    let sanitize_config = SanitizeConfig {
        strip_paths: config.privacy.strip_paths,
        strip_env_vars: config.privacy.strip_env_vars,
        exclude_patterns: config.privacy.exclude_patterns.clone(),
    };
    sanitize_session(session, &sanitize_config);
}
