use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_api::UploadRequest;
use opensession_api_client::retry::{retry_post, RetryConfig};
use opensession_api_client::ApiClient;
use opensession_core::sanitize::{sanitize_session, SanitizeConfig};
use opensession_core::session::{
    build_git_storage_meta_json_with_git, is_auxiliary_session, working_directory, GitMeta,
};
use opensession_core::Session;
use opensession_git_native::{
    branch_ledger_ref, extract_git_context, resolve_ledger_branch, PruneStats,
};
use opensession_local_db::LocalDb;
use opensession_parsers::parse_with_default_parsers;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::{DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode};
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
            match storage.prune_by_age_at_ref(&repo_root, &ref_name, keep_days) {
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

/// Run the scheduler loop: receives file change events, debounces, parses, and uploads.
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

    // Pending changes: path -> when we last saw a change
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let retention_schedule = resolve_git_retention_schedule(&config);
    let mut next_retention_run = retention_schedule.map(|(_, interval)| Instant::now() + interval);

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
                            "Manual mode, indexing locally without auto-upload: {}",
                            path.display()
                        );
                    }
                    if let Err(e) = process_file(
                        &path,
                        &config,
                        &db,
                        &mut repo_registry,
                        should_auto_upload(&effective_mode),
                    )
                    .await
                    {
                        error!("Failed to process {}: {:#}", path.display(), e);
                    }
                }

                if let (Some((keep_days, interval)), Some(next_at)) =
                    (retention_schedule, next_retention_run)
                {
                    if now >= next_at {
                        if let Err(e) = run_git_retention_once(&repo_registry, keep_days) {
                            warn!("Git retention scan failed: {e}");
                        }
                        next_retention_run = Some(now + interval);
                    }
                }

            }

            // Shutdown signal
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
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

/// Process a single file: parse, store in local DB, sanitize, upload.
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

    store_locally(&session, path, db)?;

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

    upload_to_server(
        &session,
        &effective_config,
        db,
        git_store
            .as_ref()
            .and_then(|stored| stored.body_url.as_deref()),
    )
    .await
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

fn store_locally(session: &Session, path: &Path, db: &LocalDb) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let git = session_cwd(session)
        .map(extract_git_context)
        .unwrap_or_default();
    let local_git = opensession_local_db::git::GitContext {
        remote: git.remote.clone(),
        branch: git.branch.clone(),
        commit: git.commit.clone(),
        repo_name: git.repo_name.clone(),
    };

    db.upsert_local_session(session, &path_str, &local_git)?;
    Ok(())
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

async fn upload_to_server(
    session: &Session,
    config: &DaemonConfig,
    db: &LocalDb,
    body_url: Option<&str>,
) -> Result<()> {
    let mut api = ApiClient::new(&config.server.url, Duration::from_secs(60))?;
    api.set_auth(config.server.api_key.clone());

    info!(
        "Uploading session {} to {}",
        session.session_id,
        api.base_url()
    );

    let upload_body = serde_json::to_value(&UploadRequest {
        session: session.clone(),
        body_url: body_url.map(String::from),
        linked_session_ids: None,
        git_remote: None,
        git_branch: None,
        git_commit: None,
        git_repo_name: None,
        pr_number: None,
        pr_url: None,
        score_plugin: None,
    })?;

    let retry_cfg = RetryConfig {
        max_retries: config.daemon.max_retries as usize,
        delays: (0..config.daemon.max_retries)
            .map(|i| 1u64 << i.min(4))
            .collect(),
    };

    let url = format!("{}/api/sessions", api.base_url());
    let response = retry_post(
        api.reqwest_client(),
        &url,
        api.auth_token(),
        &upload_body,
        &retry_cfg,
    )
    .await?;

    let status = response.status();
    if status.is_success() {
        info!("Uploaded session: {}", session.session_id);
        db.mark_synced(&session.session_id)?;
    } else if status.is_client_error() {
        let body = response.text().await.unwrap_or_default();
        error!("Upload rejected (HTTP {}): {}", status, body);
    } else {
        let body = response.text().await.unwrap_or_default();
        error!("Upload failed (HTTP {}): {}", status, body);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_core::{Agent, Session};
    use serde_json::json;
    use std::collections::HashMap;

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
}
