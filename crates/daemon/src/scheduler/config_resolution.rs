use std::time::Duration;

use opensession_core::Session;

use super::helpers::session_cwd;
use crate::config::{DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode};

pub(super) fn resolve_publish_mode(settings: &DaemonSettings) -> PublishMode {
    settings.publish_on.clone()
}

pub(super) fn should_auto_upload(mode: &PublishMode) -> bool {
    !matches!(mode, PublishMode::Manual)
}

pub(super) fn resolve_git_retention_schedule(config: &DaemonConfig) -> Option<(u32, Duration)> {
    if config.git_storage.method == GitStorageMethod::Sqlite {
        return None;
    }
    if !config.git_storage.retention.enabled {
        return None;
    }

    let keep_days = config.git_storage.retention.keep_days;
    let interval_secs = config.git_storage.retention.interval_secs.max(60);
    Some((keep_days, Duration::from_secs(interval_secs)))
}

pub(super) fn resolve_lifecycle_schedule(config: &DaemonConfig) -> Option<Duration> {
    if !config.lifecycle.enabled {
        return None;
    }
    Some(Duration::from_secs(
        config.lifecycle.cleanup_interval_secs.max(60),
    ))
}

pub(super) fn resolve_effective_config(session: &Session, config: &DaemonConfig) -> DaemonConfig {
    if let Some(cwd) = session_cwd(session) {
        if let Some(repo_root) = crate::config::find_repo_root(cwd) {
            if let Some(project) = crate::config::load_effective_project_config(&repo_root) {
                return crate::config::merge_project_config(config, &project);
            }
        }
    }

    config.clone()
}
