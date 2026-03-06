use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_core::Session;
use opensession_core::sanitize::{SanitizeConfig, sanitize_session};
use opensession_core::session::{
    GitMeta, build_git_storage_meta_json_with_git, interaction_compressed_session,
    is_auxiliary_session, working_directory,
};
use opensession_git_native::{
    PruneStats, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord, branch_ledger_ref,
    extract_git_context, resolve_ledger_branch,
};
use opensession_local_db::LocalDb;
use opensession_parsers::parse_with_default_parsers;
use opensession_summary::{GitSummaryRequest, summarize_session};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::{
    DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode, SessionDefaultView,
};
use crate::repo_registry::RepoRegistry;
use crate::watcher::FileChangeEvent;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract the working directory from session context attributes.
fn session_cwd(session: &Session) -> Option<&str> {
    working_directory(session)
}

/// Build a JSON metadata blob for git storage from a session.
fn build_session_meta_json(session: &Session, git: Option<&GitMeta>) -> Vec<u8> {
    build_git_storage_meta_json_with_git(session, git)
}

fn session_to_hail_jsonl_bytes(session: &Session) -> Option<Vec<u8>> {
    match session.to_jsonl() {
        Ok(jsonl) => Some(jsonl.into_bytes()),
        Err(e) => {
            warn!(
                "Failed to serialize session {} to HAIL JSONL: {}",
                session.session_id, e
            );
            None
        }
    }
}

/// Resolve the effective publish mode from canonical runtime config.
fn resolve_publish_mode(settings: &DaemonSettings) -> PublishMode {
    settings.publish_on.clone()
}

fn should_auto_upload(mode: &PublishMode) -> bool {
    !matches!(mode, PublishMode::Manual)
}

/// Resolve retention schedule for git-native session pruning.
fn resolve_git_retention_schedule(config: &DaemonConfig) -> Option<(u32, Duration)> {
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

fn resolve_lifecycle_schedule(config: &DaemonConfig) -> Option<Duration> {
    if !config.lifecycle.enabled {
        return None;
    }
    Some(Duration::from_secs(
        config.lifecycle.cleanup_interval_secs.max(60),
    ))
}

fn run_lifecycle_cleanup_on_start(config: &DaemonConfig, db: &LocalDb, registry: &RepoRegistry) {
    if resolve_lifecycle_schedule(config).is_none() {
        return;
    }
    if let Err(e) = run_lifecycle_cleanup_once(config, db, registry) {
        warn!("Lifecycle startup cleanup failed: {e}");
    }
}

fn run_git_retention_once(registry: &RepoRegistry, keep_days: u32) -> Result<()> {
    let repo_roots = registry.repo_roots();
    if repo_roots.is_empty() {
        debug!("Git retention: no tracked repositories");
        return Ok(());
    }

    let storage = opensession_git_native::NativeGitStorage;
    for repo_root in repo_roots {
        let refs = list_branch_ledger_refs(&repo_root);
        if refs.is_empty() {
            continue;
        }
        for ref_name in refs {
            let prune_result = storage.prune_by_age_at_ref(&repo_root, &ref_name, keep_days);
            match prune_result {
                Ok(PruneStats {
                    scanned_sessions,
                    expired_sessions,
                    rewritten,
                }) => {
                    if rewritten {
                        info!(
                            repo = %repo_root.display(),
                            ref_name,
                            keep_days,
                            scanned_sessions,
                            expired_sessions,
                            "Git retention: pruned expired sessions"
                        );
                    } else {
                        debug!(
                            repo = %repo_root.display(),
                            ref_name,
                            keep_days,
                            scanned_sessions,
                            "Git retention: no expired sessions"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        repo = %repo_root.display(),
                        ref_name,
                        keep_days,
                        "Git retention failed: {e}"
                    );
                }
            }
        }
    }

    Ok(())
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

fn run_lifecycle_cleanup_once(
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
            if let Err(e) = delete_result {
                warn!(
                    repo = %repo_root.display(),
                    session_id,
                    "Lifecycle cleanup: failed to delete hidden-ref summary for expired session: {e}"
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
            Err(e) => {
                warn!(
                    repo = %repo_root.display(),
                    "Lifecycle cleanup: hidden-ref summary pruning failed: {e}"
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

fn list_branch_ledger_refs(repo_root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("for-each-ref")
        .arg("--format=%(refname)")
        .arg(opensession_git_native::BRANCH_LEDGER_REF_PREFIX)
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

fn commit_shas_from_reflog(repo_root: &Path, start_ts: i64, end_ts: i64) -> Vec<String> {
    let git_dir_output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--git-dir")
        .output();
    let Ok(git_dir_output) = git_dir_output else {
        return Vec::new();
    };
    if !git_dir_output.status.success() {
        return Vec::new();
    }
    let git_dir = String::from_utf8_lossy(&git_dir_output.stdout)
        .trim()
        .to_string();
    if git_dir.is_empty() {
        return Vec::new();
    }
    let git_dir_path = if Path::new(&git_dir).is_absolute() {
        PathBuf::from(git_dir)
    } else {
        repo_root.join(git_dir)
    };
    let reflog_path = git_dir_path.join("logs").join("HEAD");
    let raw = std::fs::read_to_string(&reflog_path);
    let Ok(raw) = raw else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut commits = Vec::new();
    for line in raw.lines() {
        let Some((left, _msg)) = line.split_once('\t') else {
            continue;
        };
        let mut pieces = left.split_whitespace();
        let _old = pieces.next();
        let new = pieces.next();
        let Some(new_sha) = new else {
            continue;
        };
        if new_sha.len() < 7 || !new_sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let mut tail = left.split_whitespace().rev();
        let _tz = tail.next();
        let ts_raw = tail.next();
        let Some(ts_raw) = ts_raw else {
            continue;
        };
        let Ok(ts) = ts_raw.parse::<i64>() else {
            continue;
        };
        if ts < start_ts || ts > end_ts {
            continue;
        }
        if seen.insert(new_sha.to_string()) {
            commits.push(new_sha.to_string());
        }
    }
    commits
}

fn collect_commit_shas_for_session(repo_root: &Path, session: &Session) -> Vec<String> {
    let created = session.context.created_at.timestamp();
    let updated = session.context.updated_at.timestamp();
    let start = created.min(updated);
    let end = created.max(updated);

    let mut commits = commit_shas_from_reflog(repo_root, start, end);
    if commits.is_empty() {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .arg("rev-parse")
            .arg("HEAD")
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !head.is_empty() {
                    commits.push(head);
                }
            }
        }
    }
    commits
}

/// Run the scheduler loop: receives file change events, debounces, parses, and marks share-ready state.
pub async fn run_scheduler(
    config: DaemonConfig,
    mut rx: mpsc::UnboundedReceiver<FileChangeEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    db: std::sync::Arc<LocalDb>,
) {
    let debounce_duration = Duration::from_secs(config.daemon.debounce_secs);

    let effective_mode = resolve_publish_mode(&config.daemon);
    let mut repo_registry = match RepoRegistry::load_default() {
        Ok(registry) => registry,
        Err(e) => {
            warn!("failed to load repo registry: {e}");
            RepoRegistry::default()
        }
    };

    // Run lifecycle cleanup once at startup before interval-driven cycles.
    run_lifecycle_cleanup_on_start(&config, &db, &repo_registry);

    // Pending changes: path -> when we last saw a change
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let retention_schedule = resolve_git_retention_schedule(&config);
    let mut next_retention_run = retention_schedule.map(|(_, interval)| Instant::now() + interval);
    let lifecycle_interval = resolve_lifecycle_schedule(&config);
    let mut next_lifecycle_run = lifecycle_interval.map(|interval| Instant::now() + interval);

    loop {
        tokio::select! {
            // Receive new file change events
            Some(event) = rx.recv() => {
                debug!("Scheduling: {:?}", event.path.display());
                pending.insert(event.path, Instant::now());
            }

            // Periodic tick to check for debounced items
            _ = tick.tick() => {
                let now = Instant::now();
                let effective_debounce = match effective_mode {
                    PublishMode::Realtime => Duration::from_millis(config.daemon.realtime_debounce_ms),
                    _ => debounce_duration,
                };

                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, last_change)| now.duration_since(**last_change) >= effective_debounce)
                    .map(|(path, _)| path.clone())
                    .collect();

                for path in ready {
                    pending.remove(&path);
                    if matches!(effective_mode, PublishMode::Manual) {
                        debug!(
                            "Manual mode, indexing locally without auto-publish: {}",
                            path.display()
                        );
                    }
                    let process_result = process_file(
                        &path,
                        &config,
                        &db,
                        &mut repo_registry,
                        should_auto_upload(&effective_mode),
                    )
                    .await;
                    if let Err(e) = process_result {
                        error!("Failed to process {}: {:#}", path.display(), e);
                    }
                }

                if let (Some((keep_days, interval)), Some(next_at)) =
                    (retention_schedule, next_retention_run)
                {
                    if now >= next_at {
                        let retention_result = run_git_retention_once(&repo_registry, keep_days);
                        if let Err(e) = retention_result {
                            warn!("Git retention scan failed: {e}");
                        }
                        next_retention_run = Some(now + interval);
                    }
                }

                if let (Some(interval), Some(next_at)) = (lifecycle_interval, next_lifecycle_run) {
                    if now >= next_at {
                        let cleanup_result =
                            run_lifecycle_cleanup_once(&config, &db, &repo_registry);
                        if let Err(e) = cleanup_result {
                            warn!("Lifecycle cleanup failed: {e}");
                        }
                        next_lifecycle_run = Some(now + interval);
                    }
                }

            }

            // Shutdown signal
            _ = shutdown.changed() => {
                let should_shutdown = {
                    let borrowed = shutdown.borrow();
                    *borrowed
                };
                if should_shutdown {
                    info!("Scheduler shutting down");
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// process_file: orchestrator + helpers
// ---------------------------------------------------------------------------

/// Process a single file: parse, store in local DB, sanitize, and prepare share state.
async fn process_file(
    path: &PathBuf,
    config: &DaemonConfig,
    db: &LocalDb,
    repo_registry: &mut RepoRegistry,
    auto_upload: bool,
) -> Result<()> {
    if was_already_uploaded(path, db)? {
        return Ok(());
    }

    let mut session = match parse_session(path)? {
        Some(s) => s,
        None => return Ok(()),
    };

    // Resolve effective config (global + project-level)
    let effective_config = resolve_effective_config(&session, config);

    if is_tool_excluded(&session, &effective_config) {
        return Ok(());
    }

    store_locally(&session, path, db, &effective_config)?;
    if let Err(error) = maybe_generate_semantic_summary(&session, db, &effective_config).await {
        warn!(
            session_id = %session.session_id,
            "semantic summary generation skipped/failed: {error}"
        );
    }

    if !auto_upload {
        return Ok(());
    }

    sanitize(&mut session, &effective_config);

    let git_store = maybe_git_store(&session, &effective_config);
    if let Some(ref stored) = git_store {
        if let Err(e) = repo_registry.add(&stored.repo_root) {
            warn!(
                repo = %stored.repo_root.display(),
                "failed to update repo registry: {e}"
            );
        }
    }

    mark_session_share_ready(
        &session,
        db,
        git_store
            .as_ref()
            .and_then(|stored| stored.body_url.as_deref()),
    )
}

fn resolve_effective_config(session: &Session, config: &DaemonConfig) -> DaemonConfig {
    if let Some(cwd) = session_cwd(session) {
        if let Some(repo_root) = crate::config::find_repo_root(cwd) {
            if let Some(project) = crate::config::load_effective_project_config(&repo_root) {
                return crate::config::merge_project_config(config, &project);
            }
        }
    }

    config.clone()
}

fn was_already_uploaded(path: &PathBuf, db: &LocalDb) -> Result<bool> {
    let modified: DateTime<Utc> = std::fs::metadata(path)?.modified()?.into();
    let path_str = path.to_string_lossy().to_string();
    if db.was_uploaded_after(&path_str, &modified)? {
        debug!("Skipping already-uploaded file: {}", path.display());
        return Ok(true);
    }
    Ok(false)
}

fn parse_session(path: &Path) -> Result<Option<Session>> {
    let session = match parse_with_default_parsers(path)? {
        Some(session) => session,
        None => {
            warn!("No parser for: {}", path.display());
            return Ok(None);
        }
    };
    if is_auxiliary_session(&session) {
        debug!("Skipping auxiliary session from {}", path.display());
        return Ok(None);
    }

    info!("Parsing: {}", path.display());
    Ok(Some(session))
}

fn is_tool_excluded(session: &Session, config: &DaemonConfig) -> bool {
    let excluded = config
        .privacy
        .exclude_tools
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&session.agent.tool));

    if excluded {
        info!(
            "Excluding tool '{}': source file excluded by config",
            session.agent.tool,
        );
    }
    excluded
}

fn store_locally(
    session: &Session,
    path: &Path,
    db: &LocalDb,
    config: &DaemonConfig,
) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let local_session = if matches!(
        config.daemon.session_default_view,
        SessionDefaultView::Compressed
    ) {
        interaction_compressed_session(session)
    } else {
        session.clone()
    };

    let git = session_cwd(&local_session)
        .map(extract_git_context)
        .unwrap_or_default();
    let local_git = opensession_local_db::git::GitContext {
        remote: git.remote.clone(),
        branch: git.branch.clone(),
        commit: git.commit.clone(),
        repo_name: git.repo_name.clone(),
    };

    db.upsert_local_session(&local_session, &path_str, &local_git)?;
    // Keep original source bytes so summary/vector rebuild can survive source-file cleanup.
    match std::fs::read(path) {
        Ok(body) => {
            if let Err(error) = db.cache_body(&session.session_id, &body) {
                warn!(
                    "Failed to cache source body for session {}: {}",
                    session.session_id, error
                );
            }
        }
        Err(error) => {
            warn!(
                "Failed to read source file for session {} while caching body: {}",
                session.session_id, error
            );
        }
    }
    Ok(())
}

async fn maybe_generate_semantic_summary(
    session: &Session,
    db: &LocalDb,
    config: &DaemonConfig,
) -> Result<()> {
    let settings = &config.summary;
    if !settings.should_generate_on_session_save() {
        return Ok(());
    }
    if settings.storage.backend == opensession_runtime_config::SummaryStorageBackend::None {
        return Ok(());
    }
    if !settings.is_configured() {
        return Ok(());
    }

    let git_request = if settings.allows_git_changes_fallback() {
        session_cwd(session).and_then(|cwd| {
            crate::config::find_repo_root(cwd).map(|repo_root| GitSummaryRequest {
                repo_root,
                commit: extract_git_context(cwd).commit,
            })
        })
    } else {
        None
    };

    let artifact = summarize_session(session, settings, git_request.as_ref())
        .await
        .map_err(anyhow::Error::msg)?;

    match settings.storage.backend {
        opensession_runtime_config::SummaryStorageBackend::LocalDb => {
            let summary_json = serde_json::to_string(&artifact.summary)?;
            let source_details_json = if artifact.source_details.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&artifact.source_details)?)
            };
            let diff_tree_json = if artifact.diff_tree.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&artifact.diff_tree)?)
            };
            let generated_at = chrono::Utc::now().to_rfc3339();
            let provider = enum_label(&artifact.provider);
            let source_kind = enum_label(&artifact.source_kind);
            let generation_kind = enum_label(&artifact.generation_kind);
            let model = if artifact.model.trim().is_empty() {
                None
            } else {
                Some(artifact.model.clone())
            };
            let prompt_fingerprint = if artifact.prompt_fingerprint.trim().is_empty() {
                None
            } else {
                Some(artifact.prompt_fingerprint)
            };

            db.upsert_session_semantic_summary(
                &opensession_local_db::SessionSemanticSummaryUpsert {
                    session_id: &session.session_id,
                    summary_json: &summary_json,
                    generated_at: &generated_at,
                    provider: &provider,
                    model: model.as_deref(),
                    source_kind: &source_kind,
                    generation_kind: &generation_kind,
                    prompt_fingerprint: prompt_fingerprint.as_deref(),
                    source_details_json: source_details_json.as_deref(),
                    diff_tree_json: diff_tree_json.as_deref(),
                    error: artifact.error.as_deref(),
                },
            )?;
        }
        opensession_runtime_config::SummaryStorageBackend::HiddenRef => {
            let cwd = session_cwd(session)
                .ok_or_else(|| anyhow::anyhow!("session working directory is missing"))?;
            let repo_root = crate::config::find_repo_root(cwd)
                .ok_or_else(|| anyhow::anyhow!("failed to resolve git repo root"))?;
            let summary_value = serde_json::to_value(&artifact.summary)?;
            let source_details = serde_json::to_value(&artifact.source_details)?;
            let diff_tree_value = serde_json::to_value(&artifact.diff_tree)?;
            let diff_tree = diff_tree_value.as_array().cloned().unwrap_or_default();
            let record = SessionSummaryLedgerRecord {
                session_id: session.session_id.clone(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                provider: enum_label(&artifact.provider),
                model: (!artifact.model.trim().is_empty()).then_some(artifact.model.clone()),
                source_kind: enum_label(&artifact.source_kind),
                generation_kind: enum_label(&artifact.generation_kind),
                prompt_fingerprint: (!artifact.prompt_fingerprint.trim().is_empty())
                    .then_some(artifact.prompt_fingerprint),
                summary: summary_value,
                source_details,
                diff_tree,
                error: artifact.error.clone(),
            };
            opensession_git_native::NativeGitStorage
                .store_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &record)
                .map_err(anyhow::Error::msg)?;
        }
        opensession_runtime_config::SummaryStorageBackend::None => {}
    }

    Ok(())
}

fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .ok()
        .map(|raw| raw.trim_matches('"').to_string())
        .filter(|raw| !raw.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn sanitize(session: &mut Session, config: &DaemonConfig) {
    let sanitize_config = SanitizeConfig {
        strip_paths: config.privacy.strip_paths,
        strip_env_vars: config.privacy.strip_env_vars,
        exclude_patterns: config.privacy.exclude_patterns.clone(),
    };
    sanitize_session(session, &sanitize_config);
}

/// Store a session to the local git-native branch when Git-Native mode is enabled.
/// Returns the body_url (raw content URL) on success, or None on failure/not configured.
struct GitStoreOutcome {
    body_url: Option<String>,
    repo_root: PathBuf,
}

fn maybe_git_store(session: &Session, config: &DaemonConfig) -> Option<GitStoreOutcome> {
    if config.git_storage.method == GitStorageMethod::Sqlite {
        return None;
    }

    let cwd = session_cwd(session)?;
    let repo_root = crate::config::find_repo_root(cwd)?;
    let git_ctx = extract_git_context(cwd);
    let branch = resolve_ledger_branch(git_ctx.branch.as_deref(), git_ctx.commit.as_deref());
    let ref_name = branch_ledger_ref(&branch);
    let commit_shas = collect_commit_shas_for_session(&repo_root, session);

    let hail_jsonl = session_to_hail_jsonl_bytes(session)?;
    let git_meta = GitMeta {
        remote: git_ctx.remote.clone(),
        repo_name: git_ctx.repo_name.clone(),
        branch: Some(branch),
        head: git_ctx.commit.clone(),
        commits: commit_shas.clone(),
    };
    let meta_json = build_session_meta_json(session, Some(&git_meta));

    let storage = opensession_git_native::NativeGitStorage;
    match storage.store_session_at_ref(
        &repo_root,
        &ref_name,
        &session.session_id,
        &hail_jsonl,
        &meta_json,
        &commit_shas,
    ) {
        Ok(stored) => {
            info!(
                "Stored session {} to git ref {} at {}",
                session.session_id, stored.ref_name, stored.hail_path
            );
            // Try to generate raw URL from git remote
            let body_url = git_ctx.remote.as_ref().map(|remote| {
                opensession_git_native::generate_raw_url(
                    remote,
                    &stored.commit_id,
                    &stored.hail_path,
                )
            });
            Some(GitStoreOutcome {
                body_url,
                repo_root,
            })
        }
        Err(e) => {
            warn!(
                "Git-native store failed for session {}: {}",
                session.session_id, e
            );
            None
        }
    }
}

fn mark_session_share_ready(session: &Session, db: &LocalDb, body_url: Option<&str>) -> Result<()> {
    if let Some(url) = body_url {
        info!(
            "Session {} stored in git-native ledger and share-ready ({})",
            session.session_id, url
        );
    } else {
        info!(
            "Session {} indexed locally. Share with CLI quick flow: opensession share os://src/local/<sha256> --quick",
            session.session_id
        );
    }
    db.mark_synced(&session.session_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_core::{Agent, Content, Event, EventType, Session};
    use opensession_git_native::{
        NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord,
    };
    use opensession_runtime_config::{SummaryProvider, SummaryStorageBackend, SummaryTriggerMode};
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::tempdir;

    /// Helper: build a minimal Session with the given context attributes.
    fn make_session_with_attrs(attrs: HashMap<String, serde_json::Value>) -> Session {
        let mut s = Session::new(
            "test-session-id".into(),
            Agent {
                provider: "anthropic".into(),
                model: "claude-opus-4-6".into(),
                tool: "claude-code".into(),
                tool_version: None,
            },
        );
        s.context.attributes = attrs;
        s
    }

    fn make_interaction_fixture_session(session_id: &str) -> Session {
        let mut session = Session::new(
            session_id.to_string(),
            Agent {
                provider: "anthropic".into(),
                model: "claude-opus-4-6".into(),
                tool: "claude-code".into(),
                tool_version: None,
            },
        );
        session.events = vec![
            Event {
                event_id: format!("{session_id}-user"),
                timestamp: Utc::now(),
                event_type: EventType::UserMessage,
                task_id: None,
                content: Content::text("hello"),
                duration_ms: None,
                attributes: HashMap::new(),
            },
            Event {
                event_id: format!("{session_id}-tool"),
                timestamp: Utc::now(),
                event_type: EventType::ToolCall {
                    name: "write_file".to_string(),
                },
                task_id: None,
                content: Content::text(""),
                duration_ms: None,
                attributes: HashMap::new(),
            },
        ];
        session.recompute_stats();
        session
    }

    fn init_git_repo(path: &Path) {
        let status = Command::new("git")
            .arg("init")
            .current_dir(path)
            .status()
            .expect("git init should run");
        assert!(status.success(), "git init should succeed");

        let status = Command::new("git")
            .args(["config", "user.email", "test@opensession.local"])
            .current_dir(path)
            .status()
            .expect("git config user.email should run");
        assert!(status.success(), "git config user.email should succeed");

        let status = Command::new("git")
            .args(["config", "user.name", "OpenSession Tests"])
            .current_dir(path)
            .status()
            .expect("git config user.name should run");
        assert!(status.success(), "git config user.name should succeed");
    }

    #[test]
    fn test_session_cwd_from_cwd_key() {
        let mut attrs = HashMap::new();
        attrs.insert("cwd".into(), json!("/home/user/project"));
        let session = make_session_with_attrs(attrs);
        assert_eq!(session_cwd(&session), Some("/home/user/project"));
    }

    #[test]
    fn test_session_cwd_from_working_directory() {
        let mut attrs = HashMap::new();
        attrs.insert("working_directory".into(), json!("/tmp/work"));
        let session = make_session_with_attrs(attrs);
        assert_eq!(session_cwd(&session), Some("/tmp/work"));
    }

    #[test]
    fn test_session_cwd_prefers_cwd_over_working_directory() {
        let mut attrs = HashMap::new();
        attrs.insert("cwd".into(), json!("/preferred"));
        attrs.insert("working_directory".into(), json!("/fallback"));
        let session = make_session_with_attrs(attrs);
        assert_eq!(session_cwd(&session), Some("/preferred"));
    }

    #[test]
    fn test_session_cwd_missing() {
        let session = make_session_with_attrs(HashMap::new());
        assert_eq!(session_cwd(&session), None);
    }

    #[test]
    fn test_session_cwd_non_string_value_returns_none() {
        let mut attrs = HashMap::new();
        attrs.insert("cwd".into(), json!(42));
        let session = make_session_with_attrs(attrs);
        assert_eq!(session_cwd(&session), None);
    }

    #[test]
    fn test_build_session_meta_json_with_title() {
        let mut session = make_session_with_attrs(HashMap::new());
        session.context.title = Some("My Session Title".into());

        let bytes = build_session_meta_json(&session, None);
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed["session_id"], "test-session-id");
        assert_eq!(parsed["schema_version"], 2);
        assert_eq!(parsed["title"], "My Session Title");
        assert_eq!(parsed["tool"], "claude-code");
        assert_eq!(parsed["model"], "claude-opus-4-6");
        assert!(parsed["stats"].is_object());
    }

    #[test]
    fn test_build_session_meta_json_no_title() {
        let session = make_session_with_attrs(HashMap::new());

        let bytes = build_session_meta_json(&session, None);
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed["session_id"], "test-session-id");
        assert!(parsed["title"].is_null());
        assert_eq!(parsed["tool"], "claude-code");
        assert_eq!(parsed["model"], "claude-opus-4-6");
    }

    #[test]
    fn test_build_session_meta_json_includes_git_block() {
        let session = make_session_with_attrs(HashMap::new());
        let git = GitMeta {
            remote: Some("git@github.com:org/repo.git".to_string()),
            repo_name: Some("org/repo".to_string()),
            branch: Some("feature/x".to_string()),
            head: Some("abcd1234".to_string()),
            commits: vec!["abcd1234".to_string()],
        };

        let bytes = build_session_meta_json(&session, Some(&git));
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["schema_version"], 2);
        assert_eq!(parsed["git"]["repo_name"], "org/repo");
        assert_eq!(parsed["git"]["head"], "abcd1234");
    }

    #[test]
    fn test_session_to_hail_jsonl_bytes_uses_header_event_stats_lines() {
        let mut session = make_session_with_attrs(HashMap::new());
        session.events.push(opensession_core::Event {
            event_id: "e1".into(),
            timestamp: Utc::now(),
            event_type: opensession_core::EventType::UserMessage,
            task_id: None,
            content: opensession_core::Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        });
        session.recompute_stats();

        let body = session_to_hail_jsonl_bytes(&session).expect("serialize HAIL JSONL");
        let text = String::from_utf8(body).expect("jsonl must be utf-8");
        let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 3, "expected header/event/stats lines");

        let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header["type"], "header");
        let event: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(event["type"], "event");
        let stats: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(stats["type"], "stats");
    }

    #[test]
    fn test_resolve_publish_mode_auto_publish_true() {
        let settings = DaemonSettings {
            auto_publish: true,
            publish_on: PublishMode::SessionEnd,
            ..Default::default()
        };
        assert_eq!(resolve_publish_mode(&settings), PublishMode::SessionEnd);
    }

    #[test]
    fn test_resolve_publish_mode_auto_publish_false_manual() {
        let settings = DaemonSettings {
            auto_publish: false,
            publish_on: PublishMode::Manual,
            ..Default::default()
        };
        assert_eq!(resolve_publish_mode(&settings), PublishMode::Manual);
    }

    #[test]
    fn test_resolve_publish_mode_uses_publish_on_even_when_auto_publish_false() {
        let settings = DaemonSettings {
            auto_publish: false,
            publish_on: PublishMode::Realtime,
            ..Default::default()
        };
        assert_eq!(resolve_publish_mode(&settings), PublishMode::Realtime);
    }

    #[test]
    fn test_should_auto_upload_is_false_for_manual_mode() {
        assert!(!should_auto_upload(&PublishMode::Manual));
    }

    #[test]
    fn test_should_auto_upload_is_true_for_session_end_and_realtime() {
        assert!(should_auto_upload(&PublishMode::SessionEnd));
        assert!(should_auto_upload(&PublishMode::Realtime));
    }

    #[test]
    fn test_resolve_git_retention_schedule_disabled_by_default() {
        let config = DaemonConfig::default();
        assert!(resolve_git_retention_schedule(&config).is_none());
    }

    #[test]
    fn test_resolve_git_retention_schedule_enabled_native_mode() {
        let mut config = DaemonConfig::default();
        config.git_storage.method = GitStorageMethod::Native;
        config.git_storage.retention.enabled = true;
        config.git_storage.retention.keep_days = 14;
        config.git_storage.retention.interval_secs = 120;

        let (keep_days, interval) =
            resolve_git_retention_schedule(&config).expect("retention should be enabled");
        assert_eq!(keep_days, 14);
        assert_eq!(interval, Duration::from_secs(120));
    }

    #[test]
    fn test_resolve_git_retention_schedule_enforces_min_interval() {
        let mut config = DaemonConfig::default();
        config.git_storage.method = GitStorageMethod::Native;
        config.git_storage.retention.enabled = true;
        config.git_storage.retention.interval_secs = 0;

        let (_, interval) =
            resolve_git_retention_schedule(&config).expect("retention should be enabled");
        assert_eq!(interval, Duration::from_secs(60));
    }

    #[test]
    fn test_resolve_lifecycle_schedule_honors_enabled_and_min_interval() {
        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = false;
        assert!(resolve_lifecycle_schedule(&config).is_none());

        config.lifecycle.enabled = true;
        config.lifecycle.cleanup_interval_secs = 12;
        assert_eq!(
            resolve_lifecycle_schedule(&config),
            Some(Duration::from_secs(60))
        );

        config.lifecycle.cleanup_interval_secs = 120;
        assert_eq!(
            resolve_lifecycle_schedule(&config),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn test_run_lifecycle_cleanup_on_start_runs_immediately() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let mut expired = make_interaction_fixture_session("startup-expired-session");
        expired.context.created_at = Utc::now() - chrono::Duration::days(90);
        expired.context.updated_at = expired.context.created_at;
        db.upsert_local_session(
            &expired,
            "/tmp/startup-expired-session.jsonl",
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert expired session");

        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = true;
        config.lifecycle.session_ttl_days = 30;
        config.lifecycle.summary_ttl_days = 30;
        config.lifecycle.cleanup_interval_secs = 3600;

        run_lifecycle_cleanup_on_start(&config, &db, &RepoRegistry::default());

        assert!(
            db.get_session_by_id("startup-expired-session")
                .expect("query expired session")
                .is_none(),
            "expired session should be deleted during startup lifecycle cleanup"
        );
    }

    #[test]
    fn test_run_lifecycle_cleanup_deletes_expired_sessions_and_hidden_ref_summaries() {
        let tmp = tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");
        init_git_repo(&repo_root);

        let db_path = tmp.path().join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");
        let mut expired = make_interaction_fixture_session("expired-session");
        expired.context.created_at = Utc::now() - chrono::Duration::days(90);
        expired.context.updated_at = expired.context.created_at;
        expired.context.attributes.insert(
            "working_directory".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        db.upsert_local_session(
            &expired,
            "/tmp/expired-session.jsonl",
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert expired session");

        let mut active = make_interaction_fixture_session("active-session");
        active.context.attributes.insert(
            "working_directory".to_string(),
            json!(repo_root.to_string_lossy().to_string()),
        );
        db.upsert_local_session(
            &active,
            "/tmp/active-session.jsonl",
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert active session");

        let storage = NativeGitStorage;
        storage
            .store_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                &SessionSummaryLedgerRecord {
                    session_id: "expired-session".to_string(),
                    generated_at: "2026-01-01T00:00:00Z".to_string(),
                    provider: "codex_exec".to_string(),
                    model: None,
                    source_kind: "session_signals".to_string(),
                    generation_kind: "provider".to_string(),
                    prompt_fingerprint: None,
                    summary: json!({ "changes": "expired" }),
                    source_details: json!({}),
                    diff_tree: vec![],
                    error: None,
                },
            )
            .expect("store expired summary");
        storage
            .store_summary_at_ref(
                &repo_root,
                SUMMARY_LEDGER_REF,
                &SessionSummaryLedgerRecord {
                    session_id: "active-session".to_string(),
                    generated_at: "2026-01-01T00:00:00Z".to_string(),
                    provider: "codex_exec".to_string(),
                    model: None,
                    source_kind: "session_signals".to_string(),
                    generation_kind: "provider".to_string(),
                    prompt_fingerprint: None,
                    summary: json!({ "changes": "active" }),
                    source_details: json!({}),
                    diff_tree: vec![],
                    error: None,
                },
            )
            .expect("store active summary");

        let mut registry = RepoRegistry::default();
        registry
            .add(&repo_root)
            .expect("repo registry should accept repo root");

        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = true;
        config.lifecycle.session_ttl_days = 30;
        config.lifecycle.summary_ttl_days = 10_000;
        config.lifecycle.cleanup_interval_secs = 60;

        run_lifecycle_cleanup_once(&config, &db, &registry).expect("run lifecycle cleanup");

        assert!(
            db.get_session_by_id("expired-session")
                .expect("query expired session")
                .is_none(),
            "expired session should be deleted"
        );
        assert!(
            db.get_session_by_id("active-session")
                .expect("query active session")
                .is_some(),
            "active session should remain"
        );
        assert!(
            storage
                .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "expired-session")
                .expect("load expired summary")
                .is_none(),
            "hidden-ref summary for expired session should be deleted"
        );
        assert!(
            storage
                .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "active-session")
                .expect("load active summary")
                .is_some(),
            "active session summary should remain"
        );
    }

    #[test]
    fn test_run_lifecycle_cleanup_prunes_local_summary_rows_by_ttl() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let session_old = make_interaction_fixture_session("summary-old");
        db.upsert_local_session(
            &session_old,
            "/tmp/summary-old.jsonl",
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert old summary session");
        let session_new = make_interaction_fixture_session("summary-new");
        db.upsert_local_session(
            &session_new,
            "/tmp/summary-new.jsonl",
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert new summary session");

        db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
            session_id: "summary-old",
            summary_json: r#"{"changes":"old"}"#,
            generated_at: "2020-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("insert old summary");
        db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
            session_id: "summary-new",
            summary_json: r#"{"changes":"new"}"#,
            generated_at: "2999-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("insert new summary");

        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = true;
        config.lifecycle.session_ttl_days = 10_000;
        config.lifecycle.summary_ttl_days = 30;
        config.lifecycle.cleanup_interval_secs = 60;

        run_lifecycle_cleanup_once(&config, &db, &RepoRegistry::default())
            .expect("run lifecycle cleanup");

        assert!(
            db.get_session_semantic_summary("summary-old")
                .expect("query old summary")
                .is_none(),
            "old summary should be pruned"
        );
        assert!(
            db.get_session_semantic_summary("summary-new")
                .expect("query new summary")
                .is_some(),
            "new summary should remain"
        );
    }

    #[test]
    fn test_run_lifecycle_cleanup_deletes_sessions_with_missing_source_parent_dir() {
        let tmp = tempdir().expect("tempdir");
        let db_path = tmp.path().join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let missing_parent_root = tmp.path().join("deleted-source-root");
        std::fs::create_dir_all(&missing_parent_root).expect("create missing parent root");
        let missing_parent_source = missing_parent_root.join("missing-parent.jsonl");

        let existing_parent_root = tmp.path().join("existing-source-root");
        std::fs::create_dir_all(&existing_parent_root).expect("create existing parent root");
        let existing_parent_source = existing_parent_root.join("missing-file.jsonl");

        let missing_parent_session = make_interaction_fixture_session("missing-parent-session");
        db.upsert_local_session(
            &missing_parent_session,
            missing_parent_source
                .to_str()
                .expect("missing parent source path should be utf-8"),
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert missing-parent session");

        let existing_parent_session = make_interaction_fixture_session("existing-parent-session");
        db.upsert_local_session(
            &existing_parent_session,
            existing_parent_source
                .to_str()
                .expect("existing parent source path should be utf-8"),
            &opensession_local_db::git::GitContext::default(),
        )
        .expect("upsert existing-parent session");

        std::fs::remove_dir_all(&missing_parent_root).expect("remove missing parent root");

        let mut config = DaemonConfig::default();
        config.lifecycle.enabled = true;
        config.lifecycle.session_ttl_days = 10_000;
        config.lifecycle.summary_ttl_days = 10_000;
        config.lifecycle.cleanup_interval_secs = 60;

        run_lifecycle_cleanup_once(&config, &db, &RepoRegistry::default())
            .expect("run lifecycle cleanup");

        assert!(
            db.get_session_by_id("missing-parent-session")
                .expect("query missing-parent session")
                .is_none(),
            "session should be deleted when source parent directory is gone"
        );
        assert!(
            db.get_session_by_id("existing-parent-session")
                .expect("query existing-parent session")
                .is_some(),
            "session should remain when source parent directory still exists"
        );
    }

    #[test]
    fn test_store_locally_uses_compressed_session_only_when_default_view_is_compressed() {
        let tmp = tempdir().expect("tempdir");
        let db_path = PathBuf::from(tmp.path()).join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let full_session = make_interaction_fixture_session("store-full");
        let mut full_config = DaemonConfig::default();
        full_config.daemon.session_default_view = SessionDefaultView::Full;
        store_locally(
            &full_session,
            Path::new("/tmp/store-full.jsonl"),
            &db,
            &full_config,
        )
        .expect("store full session");

        let stored_full = db
            .get_session_by_id("store-full")
            .expect("query full")
            .expect("full session exists");
        assert_eq!(stored_full.event_count, 2);

        let compressed_session = make_interaction_fixture_session("store-compressed");
        let mut compressed_config = DaemonConfig::default();
        compressed_config.daemon.session_default_view = SessionDefaultView::Compressed;
        store_locally(
            &compressed_session,
            Path::new("/tmp/store-compressed.jsonl"),
            &db,
            &compressed_config,
        )
        .expect("store compressed session");

        let stored_compressed = db
            .get_session_by_id("store-compressed")
            .expect("query compressed")
            .expect("compressed session exists");
        assert_eq!(stored_compressed.event_count, 1);
    }

    #[test]
    fn test_store_locally_caches_source_body() {
        let tmp = tempdir().expect("tempdir");
        let db_path = PathBuf::from(tmp.path()).join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");
        let session = make_interaction_fixture_session("store-cache");
        let source_path = PathBuf::from(tmp.path()).join("store-cache.jsonl");
        let source_body = b"{\"source\":\"fixture\"}\n".to_vec();
        std::fs::write(&source_path, &source_body).expect("write source fixture");

        let mut config = DaemonConfig::default();
        config.daemon.session_default_view = SessionDefaultView::Full;
        store_locally(&session, &source_path, &db, &config).expect("store cached session");

        let cached = db
            .get_cached_body("store-cache")
            .expect("query body cache")
            .expect("cache row should exist");
        assert_eq!(cached, source_body);
    }

    #[tokio::test]
    async fn test_auto_summary_runs_on_session_save_and_persists_row() {
        let tmp = tempdir().expect("tempdir");
        let db_path = PathBuf::from(tmp.path()).join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let session = make_interaction_fixture_session("summary-auto");
        let mut config = DaemonConfig::default();
        config.summary.provider.id = SummaryProvider::CodexExec;
        config.summary.storage.trigger = SummaryTriggerMode::OnSessionSave;
        config.summary.storage.backend = SummaryStorageBackend::LocalDb;

        maybe_generate_semantic_summary(&session, &db, &config)
            .await
            .expect("summary generation should not fail hard");

        let row = db
            .get_session_semantic_summary("summary-auto")
            .expect("query summary")
            .expect("summary row should exist");
        assert_eq!(row.provider, "codex_exec");
        assert_eq!(row.source_kind, "session_signals");
        assert!(!row.summary_json.trim().is_empty());
    }

    #[tokio::test]
    async fn test_auto_summary_skips_when_trigger_mode_is_manual() {
        let tmp = tempdir().expect("tempdir");
        let db_path = PathBuf::from(tmp.path()).join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let session = make_interaction_fixture_session("summary-manual");
        let mut config = DaemonConfig::default();
        config.summary.provider.id = SummaryProvider::CodexExec;
        config.summary.storage.trigger = SummaryTriggerMode::Manual;
        config.summary.storage.backend = SummaryStorageBackend::LocalDb;

        maybe_generate_semantic_summary(&session, &db, &config)
            .await
            .expect("manual trigger should no-op");

        let row = db
            .get_session_semantic_summary("summary-manual")
            .expect("query summary");
        assert!(row.is_none());
    }

    #[tokio::test]
    async fn test_auto_summary_skips_when_storage_backend_is_none() {
        let tmp = tempdir().expect("tempdir");
        let db_path = PathBuf::from(tmp.path()).join("local.db");
        let db = LocalDb::open_path(&db_path).expect("open local db");

        let session = make_interaction_fixture_session("summary-no-persist");
        let mut config = DaemonConfig::default();
        config.summary.provider.id = SummaryProvider::CodexExec;
        config.summary.storage.trigger = SummaryTriggerMode::OnSessionSave;
        config.summary.storage.backend = SummaryStorageBackend::None;

        maybe_generate_semantic_summary(&session, &db, &config)
            .await
            .expect("none persist should no-op");

        let row = db
            .get_session_semantic_summary("summary-no-persist")
            .expect("query summary");
        assert!(row.is_none());
    }
}
