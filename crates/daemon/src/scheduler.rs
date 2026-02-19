use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_api::UploadRequest;
use opensession_api_client::retry::{retry_post, RetryConfig};
use opensession_api_client::ApiClient;
use opensession_core::sanitize::{sanitize_session, SanitizeConfig};
use opensession_core::Session;
use opensession_git_native::PruneStats;
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use opensession_parsers::{all_parsers, SessionParser};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::{DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode};
use crate::watcher::FileChangeEvent;

/// Legacy state – kept only for migration from state.json.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UploadState {
    pub uploaded: HashMap<String, DateTime<Utc>>,
    #[serde(default)]
    pub offsets: HashMap<String, u64>,
}

impl UploadState {
    pub fn load(path: &PathBuf) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract the working directory from session context attributes.
fn session_cwd(session: &Session) -> Option<&str> {
    session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str())
}

/// Build a JSON metadata blob for git storage from a session.
fn build_session_meta_json(session: &Session) -> Vec<u8> {
    serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session.session_id,
        "title": session.context.title,
        "tool": session.agent.tool,
        "model": session.agent.model,
        "stats": session.stats,
    }))
    .unwrap_or_default()
    .into_bytes()
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

fn run_git_retention_once(db: &LocalDb, keep_days: u32) -> Result<()> {
    let workdirs = db.list_working_directories()?;
    if workdirs.is_empty() {
        debug!("Git retention: no working directories in local DB");
        return Ok(());
    }

    let mut repo_roots = HashSet::new();
    for wd in workdirs {
        if let Some(root) = crate::config::find_repo_root(&wd) {
            repo_roots.insert(root);
        }
    }

    if repo_roots.is_empty() {
        debug!("Git retention: no repo roots found from working directories");
        return Ok(());
    }

    let storage = opensession_git_native::NativeGitStorage;
    for repo_root in repo_roots {
        match storage.prune_by_age(&repo_root, keep_days) {
            Ok(PruneStats {
                scanned_sessions,
                expired_sessions,
                rewritten,
            }) => {
                if rewritten {
                    info!(
                        repo = %repo_root.display(),
                        keep_days,
                        scanned_sessions,
                        expired_sessions,
                        "Git retention: pruned expired sessions"
                    );
                } else {
                    debug!(
                        repo = %repo_root.display(),
                        keep_days,
                        scanned_sessions,
                        "Git retention: no expired sessions"
                    );
                }
            }
            Err(e) => {
                warn!(
                    repo = %repo_root.display(),
                    keep_days,
                    "Git retention failed: {e}"
                );
            }
        }
    }

    Ok(())
}

/// Run the scheduler loop: receives file change events, debounces, parses, and uploads.
pub async fn run_scheduler(
    config: DaemonConfig,
    mut rx: mpsc::UnboundedReceiver<FileChangeEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    db: std::sync::Arc<LocalDb>,
) {
    let debounce_duration = Duration::from_secs(config.daemon.debounce_secs);

    // Migrate from state.json if it exists
    let state_path =
        crate::config::state_file_path().unwrap_or_else(|_| PathBuf::from("state.json"));
    if let Some(legacy) = UploadState::load(&state_path) {
        if !legacy.uploaded.is_empty() {
            match db.migrate_from_state_json(&legacy.uploaded) {
                Ok(count) => {
                    info!("Migrated {count} entries from state.json to local DB");
                    // Rename the old file so we don't re-migrate
                    let bak = state_path.with_extension("json.bak");
                    if let Err(e) = std::fs::rename(&state_path, &bak) {
                        warn!("Could not rename state.json to .bak: {e}");
                    }
                }
                Err(e) => warn!("state.json migration failed: {e}"),
            }
        }
    }

    let effective_mode = resolve_publish_mode(&config.daemon);

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
                    match effective_mode {
                        PublishMode::Manual => {
                            debug!("Manual mode, skipping auto-publish: {}", path.display());
                        }
                        PublishMode::SessionEnd | PublishMode::Realtime => {
                            if let Err(e) = process_file(&path, &config, &db).await {
                                error!("Failed to process {}: {:#}", path.display(), e);
                            }
                        }
                    }
                }

                if let (Some((keep_days, interval)), Some(next_at)) =
                    (retention_schedule, next_retention_run)
                {
                    if now >= next_at {
                        if let Err(e) = run_git_retention_once(&db, keep_days) {
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
async fn process_file(path: &PathBuf, config: &DaemonConfig, db: &LocalDb) -> Result<()> {
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

    sanitize(&mut session, &effective_config);

    let body_url = maybe_git_store(&session, &effective_config);

    upload_to_server(&session, &effective_config, db, body_url.as_deref()).await
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
    let parsers = all_parsers();
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(path))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => {
            warn!("No parser for: {}", path.display());
            return Ok(None);
        }
    };

    info!("Parsing: {} ({})", path.display(), parser.name());
    Ok(Some(parser.parse(path)?))
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

    db.upsert_local_session(session, &path_str, &git)?;
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
fn maybe_git_store(session: &Session, config: &DaemonConfig) -> Option<String> {
    if config.git_storage.method == GitStorageMethod::Sqlite {
        return None;
    }

    let cwd = session_cwd(session)?;
    let repo_root = crate::config::find_repo_root(cwd)?;

    let hail_jsonl = session_to_hail_jsonl_bytes(session)?;
    let meta_json = build_session_meta_json(session);

    let storage = opensession_git_native::NativeGitStorage;
    match storage.store(&repo_root, &session.session_id, &hail_jsonl, &meta_json) {
        Ok(rel_path) => {
            info!(
                "Stored session {} to git branch at {}",
                session.session_id, rel_path
            );
            // Try to generate raw URL from git remote
            let git_ctx = extract_git_context(cwd);
            git_ctx
                .remote
                .as_ref()
                .map(|remote| opensession_git_native::generate_raw_url(remote, &rel_path))
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

        let bytes = build_session_meta_json(&session);
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed["session_id"], "test-session-id");
        assert_eq!(parsed["title"], "My Session Title");
        assert_eq!(parsed["tool"], "claude-code");
        assert_eq!(parsed["model"], "claude-opus-4-6");
        assert!(parsed["stats"].is_object());
    }

    #[test]
    fn test_build_session_meta_json_no_title() {
        let session = make_session_with_attrs(HashMap::new());

        let bytes = build_session_meta_json(&session);
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed["session_id"], "test-session-id");
        assert!(parsed["title"].is_null());
        assert_eq!(parsed["tool"], "claude-code");
        assert_eq!(parsed["model"], "claude-opus-4-6");
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
