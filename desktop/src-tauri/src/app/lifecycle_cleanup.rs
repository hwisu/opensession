use crate::app::session_summary::resolve_repo_root_from_session_row;
use crate::{
    DesktopApiResult, LIFECYCLE_CLEANUP_LOOP_STARTED, desktop_error, load_runtime_config,
    open_local_db,
};
use opensession_api::{DesktopLifecycleCleanupState, DesktopLifecycleCleanupStatusResponse};
use opensession_git_native::{BRANCH_LEDGER_REF_PREFIX, NativeGitStorage, SUMMARY_LEDGER_REF};
use opensession_local_db::{LifecycleCleanupJobRow, LocalDb, LocalSessionFilter};
use opensession_runtime_config::{DaemonConfig, GitStorageMethod};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn resolve_desktop_lifecycle_schedule(config: &DaemonConfig) -> Option<Duration> {
    if !config.lifecycle.enabled {
        return None;
    }
    Some(Duration::from_secs(
        config.lifecycle.cleanup_interval_secs.max(60),
    ))
}

fn source_parent_directory_missing(source_path: &str) -> bool {
    let path = Path::new(source_path);
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .is_some_and(|parent| !parent.exists())
}

fn list_sessions_with_missing_source_parent_dirs(db: &LocalDb) -> DesktopApiResult<Vec<String>> {
    let mut orphaned = db
        .list_session_source_paths()
        .map_err(|error| {
            desktop_error(
                "desktop.lifecycle_cleanup_list_source_paths_failed",
                500,
                "failed to list session source paths for lifecycle cleanup",
                Some(json!({ "cause": error.to_string() })),
            )
        })?
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

fn collect_desktop_lifecycle_repo_roots(db: &LocalDb) -> DesktopApiResult<BTreeSet<PathBuf>> {
    let mut roots = BTreeSet::new();
    let rows = db
        .list_sessions(&LocalSessionFilter::default())
        .map_err(|error| {
            desktop_error(
                "desktop.lifecycle_cleanup_list_sessions_failed",
                500,
                "failed to list sessions for lifecycle cleanup",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    for row in rows {
        if let Some(repo_root) = resolve_repo_root_from_session_row(&row) {
            roots.insert(repo_root);
        }
    }

    Ok(roots)
}

fn list_branch_ledger_refs(repo_root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("for-each-ref")
        .arg("--format=%(refname)")
        .arg(BRANCH_LEDGER_REF_PREFIX)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn run_desktop_git_retention_once(repo_roots: &BTreeSet<PathBuf>, keep_days: u32) {
    let storage = NativeGitStorage;
    for repo_root in repo_roots {
        for ref_name in list_branch_ledger_refs(repo_root) {
            let prune_result = storage.prune_by_age_at_ref(repo_root, &ref_name, keep_days);
            if let Err(error) = prune_result {
                eprintln!(
                    "lifecycle cleanup: failed to prune branch ledger {ref_name} in {}: {error}",
                    repo_root.display()
                );
            }
        }
    }
}

fn map_lifecycle_cleanup_state(raw: &str) -> DesktopLifecycleCleanupState {
    match raw {
        "running" => DesktopLifecycleCleanupState::Running,
        "complete" => DesktopLifecycleCleanupState::Complete,
        "failed" => DesktopLifecycleCleanupState::Failed,
        _ => DesktopLifecycleCleanupState::Idle,
    }
}

pub(crate) fn desktop_lifecycle_cleanup_status_from_db(
    db: &LocalDb,
) -> DesktopApiResult<DesktopLifecycleCleanupStatusResponse> {
    let row = db.get_lifecycle_cleanup_job().map_err(|error| {
        desktop_error(
            "desktop.lifecycle_cleanup_status_failed",
            500,
            "failed to read lifecycle cleanup status",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    let Some(row) = row else {
        return Ok(DesktopLifecycleCleanupStatusResponse {
            state: DesktopLifecycleCleanupState::Idle,
            deleted_sessions: 0,
            deleted_summaries: 0,
            message: None,
            started_at: None,
            finished_at: None,
        });
    };

    Ok(DesktopLifecycleCleanupStatusResponse {
        state: map_lifecycle_cleanup_state(&row.status),
        deleted_sessions: row.deleted_sessions,
        deleted_summaries: row.deleted_summaries,
        message: row.message,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

pub(crate) fn set_lifecycle_cleanup_job_snapshot(
    db: &LocalDb,
    payload: LifecycleCleanupJobRow,
) -> DesktopApiResult<()> {
    db.set_lifecycle_cleanup_job(&payload).map_err(|error| {
        desktop_error(
            "desktop.lifecycle_cleanup_status_failed",
            500,
            "failed to persist lifecycle cleanup status",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

pub(crate) fn run_desktop_lifecycle_cleanup_once_with_db(
    config: &DaemonConfig,
    db: &LocalDb,
) -> DesktopApiResult<()> {
    if !config.lifecycle.enabled {
        return Ok(());
    }

    let started_at = chrono::Utc::now().to_rfc3339();
    set_lifecycle_cleanup_job_snapshot(
        db,
        LifecycleCleanupJobRow {
            status: "running".to_string(),
            deleted_sessions: 0,
            deleted_summaries: 0,
            message: Some("Scanning lifecycle cleanup candidates.".to_string()),
            started_at: Some(started_at.clone()),
            finished_at: None,
        },
    )?;

    let storage = NativeGitStorage;
    let mut deleted_sessions = 0u32;
    let mut deleted_summaries = 0u32;

    let result: DesktopApiResult<Option<String>> = (|| {
        let repo_roots = collect_desktop_lifecycle_repo_roots(db)?;
        let expired_sessions = db
            .list_expired_session_ids(config.lifecycle.session_ttl_days)
            .map_err(|error| {
                desktop_error(
                    "desktop.lifecycle_cleanup_list_expired_sessions_failed",
                    500,
                    "failed to list expired sessions for lifecycle cleanup",
                    Some(json!({
                        "cause": error.to_string(),
                        "session_ttl_days": config.lifecycle.session_ttl_days
                    })),
                )
            })?;
        let expired_session_count = expired_sessions.len() as u32;
        let orphaned_sessions = list_sessions_with_missing_source_parent_dirs(db)?;
        let orphaned_session_count = orphaned_sessions.len() as u32;
        let mut sessions_to_delete = expired_sessions.into_iter().collect::<BTreeSet<_>>();
        sessions_to_delete.extend(orphaned_sessions);

        if !sessions_to_delete.is_empty() {
            set_lifecycle_cleanup_job_snapshot(
                db,
                LifecycleCleanupJobRow {
                    status: "running".to_string(),
                    deleted_sessions,
                    deleted_summaries,
                    message: Some(format!(
                        "Deleting {} lifecycle cleanup candidates.",
                        sessions_to_delete.len()
                    )),
                    started_at: Some(started_at.clone()),
                    finished_at: None,
                },
            )?;
        }

        for session_id in &sessions_to_delete {
            let row = db.get_session_by_id(session_id).map_err(|error| {
                desktop_error(
                    "desktop.lifecycle_cleanup_load_session_failed",
                    500,
                    "failed to load session for lifecycle cleanup",
                    Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                )
            })?;
            if let Some(repo_root) = row.as_ref().and_then(resolve_repo_root_from_session_row) {
                let delete_result =
                    storage.delete_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, session_id);
                match delete_result {
                    Ok(true) => {
                        deleted_summaries = deleted_summaries.saturating_add(1);
                    }
                    Ok(false) => {}
                    Err(error) => {
                        eprintln!(
                            "lifecycle cleanup: failed to delete hidden-ref summary for {} in {}: {error}",
                            session_id,
                            repo_root.display()
                        );
                    }
                }
            }

            db.delete_session(session_id).map_err(|error| {
                desktop_error(
                    "desktop.lifecycle_cleanup_delete_session_failed",
                    500,
                    "failed to delete expired session during lifecycle cleanup",
                    Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                )
            })?;
            deleted_sessions = deleted_sessions.saturating_add(1);
        }

        deleted_summaries = deleted_summaries.saturating_add(
            db.delete_expired_session_summaries(config.lifecycle.summary_ttl_days)
                .map_err(|error| {
                    desktop_error(
                        "desktop.lifecycle_cleanup_delete_summaries_failed",
                        500,
                        "failed to delete expired semantic summaries during lifecycle cleanup",
                        Some(json!({
                            "cause": error.to_string(),
                            "summary_ttl_days": config.lifecycle.summary_ttl_days
                        })),
                    )
                })?,
        );

        for repo_root in &repo_roots {
            let prune_result = storage.prune_summaries_by_age_at_ref(
                repo_root,
                SUMMARY_LEDGER_REF,
                config.lifecycle.summary_ttl_days,
            );
            match prune_result {
                Ok(stats) => {
                    deleted_summaries =
                        deleted_summaries.saturating_add(stats.expired_sessions as u32);
                }
                Err(error) => {
                    eprintln!(
                        "lifecycle cleanup: failed to prune hidden-ref summaries in {}: {error}",
                        repo_root.display()
                    );
                }
            }
        }

        if config.git_storage.method != GitStorageMethod::Sqlite
            && config.git_storage.retention.enabled
        {
            run_desktop_git_retention_once(&repo_roots, config.lifecycle.session_ttl_days);
        }

        let message = if deleted_sessions == 0 && deleted_summaries == 0 {
            Some("Lifecycle cleanup complete. No expired data needed removal.".to_string())
        } else {
            Some(format!(
                "Lifecycle cleanup complete. Deleted {deleted_sessions} sessions ({expired_session_count} TTL, {orphaned_session_count} orphaned) and removed {deleted_summaries} summaries."
            ))
        };

        Ok(message)
    })();

    match result {
        Ok(message) => {
            set_lifecycle_cleanup_job_snapshot(
                db,
                LifecycleCleanupJobRow {
                    status: "complete".to_string(),
                    deleted_sessions,
                    deleted_summaries,
                    message,
                    started_at: Some(started_at),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                },
            )?;
            Ok(())
        }
        Err(error) => {
            if let Err(persist_error) = set_lifecycle_cleanup_job_snapshot(
                db,
                LifecycleCleanupJobRow {
                    status: "failed".to_string(),
                    deleted_sessions,
                    deleted_summaries,
                    message: Some(error.message.clone()),
                    started_at: Some(started_at),
                    finished_at: Some(chrono::Utc::now().to_rfc3339()),
                },
            ) {
                eprintln!(
                    "failed to persist lifecycle cleanup failure snapshot: {}",
                    persist_error.message
                );
            }
            Err(error)
        }
    }
}

fn run_desktop_lifecycle_cleanup_once(config: &DaemonConfig) -> DesktopApiResult<()> {
    let db = open_local_db()?;
    run_desktop_lifecycle_cleanup_once_with_db(config, &db)
}

pub(crate) fn maybe_start_lifecycle_cleanup_loop() {
    let mut started = LIFECYCLE_CLEANUP_LOOP_STARTED
        .lock()
        .expect("lifecycle cleanup loop mutex poisoned");
    if *started {
        return;
    }
    *started = true;
    drop(started);

    std::thread::spawn(|| {
        loop {
            let sleep_for = match load_runtime_config() {
                Ok(config) => {
                    if let Err(error) = run_desktop_lifecycle_cleanup_once(&config) {
                        eprintln!("failed to run desktop lifecycle cleanup: {}", error.message);
                    }
                    resolve_desktop_lifecycle_schedule(&config)
                        .unwrap_or_else(|| Duration::from_secs(60))
                }
                Err(error) => {
                    eprintln!(
                        "failed to load runtime config for lifecycle cleanup: {}",
                        error.message
                    );
                    Duration::from_secs(60)
                }
            };
            std::thread::sleep(sleep_for);
        }
    });
}

#[tauri::command]
pub(crate) fn desktop_lifecycle_cleanup_status()
-> DesktopApiResult<DesktopLifecycleCleanupStatusResponse> {
    let db = open_local_db()?;
    desktop_lifecycle_cleanup_status_from_db(&db)
}
