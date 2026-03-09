use anyhow::Result;
use opensession_git_native::{PruneStats, SUMMARY_LEDGER_REF};
use opensession_local_db::LocalDb;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::config::{DaemonConfig, GitStorageMethod};
use crate::repo_registry::RepoRegistry;

use super::config_resolution::resolve_lifecycle_schedule;
use super::git_retention::run_git_retention_once;

pub(super) fn run_lifecycle_cleanup_on_start(
    config: &DaemonConfig,
    db: &LocalDb,
    registry: &RepoRegistry,
) {
    if resolve_lifecycle_schedule(config).is_none() {
        return;
    }
    if let Err(error) = run_lifecycle_cleanup_once(config, db, registry) {
        warn!("Lifecycle startup cleanup failed: {error}");
    }
}

fn resolve_repo_root_from_working_directory(cwd: Option<&str>) -> Option<PathBuf> {
    cwd.and_then(crate::config::find_repo_root)
}

fn collect_lifecycle_repo_roots(db: &LocalDb, registry: &RepoRegistry) -> Result<Vec<PathBuf>> {
    let mut deduped: HashSet<PathBuf> = registry.repo_roots().into_iter().collect();

    let filter = opensession_local_db::LocalSessionFilter {
        limit: None,
        offset: None,
        ..Default::default()
    };
    let rows = db.list_sessions(&filter)?;
    for row in rows {
        if let Some(repo_root) =
            resolve_repo_root_from_working_directory(row.working_directory.as_deref())
        {
            deduped.insert(repo_root);
        }
    }

    let mut roots = deduped.into_iter().collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn source_parent_directory_missing(source_path: &str) -> bool {
    let path = Path::new(source_path);
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .is_some_and(|parent| !parent.exists())
}

fn list_sessions_with_missing_source_parent_dirs(db: &LocalDb) -> Result<Vec<String>> {
    let mut orphaned = db
        .list_session_source_paths()?
        .into_iter()
        .filter_map(|(session_id, source_path)| {
            if source_parent_directory_missing(&source_path) {
                Some(session_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    orphaned.sort();
    orphaned.dedup();
    Ok(orphaned)
}

pub(super) fn run_lifecycle_cleanup_once(
    config: &DaemonConfig,
    db: &LocalDb,
    registry: &RepoRegistry,
) -> Result<()> {
    if !config.lifecycle.enabled {
        return Ok(());
    }

    let storage = opensession_git_native::NativeGitStorage;
    let expired_sessions = db.list_expired_session_ids(config.lifecycle.session_ttl_days)?;
    let orphaned_sessions = list_sessions_with_missing_source_parent_dirs(db)?;
    let mut sessions_to_delete = expired_sessions
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    sessions_to_delete.extend(orphaned_sessions);
    let total_sessions_to_delete = sessions_to_delete.len();
    let mut deleted_sessions = 0usize;

    for session_id in sessions_to_delete {
        let row = db.get_session_by_id(&session_id)?;
        let repo_root = resolve_repo_root_from_working_directory(
            row.as_ref()
                .and_then(|row| row.working_directory.as_deref()),
        );
        if let Some(repo_root) = repo_root {
            let delete_result =
                storage.delete_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &session_id);
            if let Err(error) = delete_result {
                warn!(
                    repo = %repo_root.display(),
                    session_id,
                    "Lifecycle cleanup: failed to delete hidden-ref summary for expired session: {error}"
                );
            }
        }
        db.delete_session(&session_id)?;
        deleted_sessions = deleted_sessions.saturating_add(1);
    }

    let deleted_local_summaries =
        db.delete_expired_session_summaries(config.lifecycle.summary_ttl_days)? as usize;

    let repo_roots = collect_lifecycle_repo_roots(db, registry)?;
    for repo_root in repo_roots {
        let prune_result = storage.prune_summaries_by_age_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            config.lifecycle.summary_ttl_days,
        );
        match prune_result {
            Ok(PruneStats {
                scanned_sessions,
                expired_sessions,
                rewritten,
            }) => {
                if rewritten {
                    info!(
                        repo = %repo_root.display(),
                        scanned_sessions,
                        expired_sessions,
                        keep_days = config.lifecycle.summary_ttl_days,
                        "Lifecycle cleanup: pruned hidden-ref summaries"
                    );
                } else {
                    debug!(
                        repo = %repo_root.display(),
                        scanned_sessions,
                        keep_days = config.lifecycle.summary_ttl_days,
                        "Lifecycle cleanup: no hidden-ref summary pruning required"
                    );
                }
            }
            Err(error) => {
                warn!(
                    repo = %repo_root.display(),
                    "Lifecycle cleanup: hidden-ref summary pruning failed: {error}"
                );
            }
        }
    }

    if config.git_storage.method != GitStorageMethod::Sqlite {
        run_git_retention_once(registry, config.lifecycle.session_ttl_days)?;
    }

    info!(
        deleted_sessions,
        total_sessions_to_delete,
        deleted_local_summaries,
        session_ttl_days = config.lifecycle.session_ttl_days,
        summary_ttl_days = config.lifecycle.summary_ttl_days,
        "Lifecycle cleanup: cycle complete"
    );
    Ok(())
}
