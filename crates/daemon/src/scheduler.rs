use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_api_client::retry::{retry_post, RetryConfig};
use opensession_api_client::ApiClient;
use opensession_api_types::UploadRequest;
use opensession_core::extract::extract_changed_paths;
use opensession_core::sanitize::{sanitize_session, SanitizeConfig};
use opensession_core::Session;
use opensession_git_native::{FileRemoval, FileSnapshot, ShadowMeta, ShadowStorage};
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

/// State for an active shadow branch.
struct ShadowState {
    session_id: String,
    repo_path: PathBuf,
    project_root: PathBuf,
    last_event_count: usize,
    checkpoint_count: usize,
    tracked_files: HashSet<String>,
    last_activity: Instant,
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

/// Resolve the effective publish mode, collapsing the deprecated `auto_publish` flag.
fn resolve_publish_mode(settings: &DaemonSettings) -> PublishMode {
    if settings.auto_publish {
        return settings.publish_on.clone();
    }
    if settings.publish_on != PublishMode::Manual {
        warn!(
            "auto_publish=false is deprecated, treating as publish_on=manual. \
             Please update your config to use publish_on = \"manual\" instead."
        );
    }
    PublishMode::Manual
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

    // Shadow state for active sessions (keyed by session file path)
    let mut shadows: HashMap<PathBuf, ShadowState> = HashMap::new();

    let condense_timeout =
        Duration::from_secs(config.git_storage.shadow_condense_timeout_secs);

    let mut tick = tokio::time::interval(Duration::from_secs(1));

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
                            if let Err(e) = process_file(&path, &config, &db, &mut shadows).await {
                                error!("Failed to process {}: {:#}", path.display(), e);
                            }
                        }
                    }
                }

                // Condense idle shadows
                if config.git_storage.shadow {
                    let to_condense: Vec<PathBuf> = shadows
                        .iter()
                        .filter(|(_, s)| now.duration_since(s.last_activity) >= condense_timeout)
                        .map(|(p, _)| p.clone())
                        .collect();

                    for path in to_condense {
                        if let Some(shadow) = shadows.remove(&path) {
                            condense_shadow(&path, &shadow);
                        }
                    }
                }
            }

            // Shutdown signal
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Scheduler shutting down");
                    // Condense all active shadows on shutdown
                    for (path, shadow) in shadows.drain() {
                        condense_shadow(&path, &shadow);
                    }
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
    shadows: &mut HashMap<PathBuf, ShadowState>,
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

    // Shadow checkpoint before sanitization (needs original paths)
    maybe_shadow_checkpoint(&session, path, &effective_config, shadows);

    sanitize(&mut session, &effective_config);

    // If git-native storage is enabled and shadow is NOT active, store to git branch
    let body_url = if effective_config.git_storage.shadow {
        None // Shadow mode: condense handles archiving
    } else {
        maybe_git_store(&session, &effective_config)
    };

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

/// If git-native storage is configured, store session to git branch.
/// Returns the body_url (raw content URL) on success, or None on failure/not configured.
fn maybe_git_store(session: &Session, config: &DaemonConfig) -> Option<String> {
    if config.git_storage.method != GitStorageMethod::Native {
        return None;
    }

    let cwd = session_cwd(session)?;
    let repo_root = crate::config::find_repo_root(cwd)?;

    let hail_jsonl = serde_json::to_vec(session).ok()?;
    let meta_json = build_session_meta_json(session);

    let storage = opensession_git_native::NativeGitStorage;
    match opensession_git_native::GitStorage::store(
        &storage,
        &repo_root,
        &session.session_id,
        &hail_jsonl,
        &meta_json,
    ) {
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

/// Create a shadow checkpoint if shadow mode is active and there are file changes.
fn maybe_shadow_checkpoint(
    session: &Session,
    path: &Path,
    config: &DaemonConfig,
    shadows: &mut HashMap<PathBuf, ShadowState>,
) {
    if !config.git_storage.shadow || config.git_storage.method != GitStorageMethod::Native {
        return;
    }

    let cwd = match session_cwd(session) {
        Some(c) => c,
        None => return,
    };

    let repo_root = match crate::config::find_repo_root(cwd) {
        Some(r) => r,
        None => return,
    };

    let project_root = PathBuf::from(cwd);
    let path_buf = path.to_path_buf();

    let shadow = shadows.entry(path_buf.clone()).or_insert_with(|| {
        // Try to recover from existing shadow meta
        let (last_event_count, checkpoint_count, tracked_files) =
            match ShadowStorage::read_meta(&repo_root, &session.session_id) {
                Ok(Some(meta)) => {
                    let tracked: HashSet<String> = meta.tracked_files.into_iter().collect();
                    // We don't know the exact event count, so start from 0 to re-scan
                    (0, meta.checkpoint_count, tracked)
                }
                _ => (0, 0, HashSet::new()),
            };

        ShadowState {
            session_id: session.session_id.clone(),
            repo_path: repo_root.clone(),
            project_root: project_root.clone(),
            last_event_count,
            checkpoint_count,
            tracked_files,
            last_activity: Instant::now(),
        }
    });

    // Extract changed paths from new events only
    let new_events = if shadow.last_event_count < session.events.len() {
        &session.events[shadow.last_event_count..]
    } else {
        return;
    };

    let (modified, deleted) = extract_changed_paths(new_events);
    if modified.is_empty() && deleted.is_empty() {
        shadow.last_activity = Instant::now();
        shadow.last_event_count = session.events.len();
        return;
    }

    // Read file contents from working directory
    let files: Vec<FileSnapshot> = modified
        .iter()
        .filter_map(|p| {
            let abs = resolve_file_path(&shadow.project_root, p);
            std::fs::read(&abs).ok().map(|content| FileSnapshot {
                rel_path: p.clone(),
                content,
            })
        })
        .collect();

    let removals: Vec<FileRemoval> = deleted
        .iter()
        .map(|p| FileRemoval {
            rel_path: p.clone(),
        })
        .collect();

    // Update tracked files
    for m in &modified {
        shadow.tracked_files.insert(m.clone());
    }
    for d in &deleted {
        shadow.tracked_files.remove(d);
    }

    let meta = ShadowMeta {
        session_id: shadow.session_id.clone(),
        created_at: Utc::now(),
        checkpoint_count: shadow.checkpoint_count + 1,
        tracked_files: shadow.tracked_files.iter().cloned().collect(),
        project_root: shadow.project_root.to_string_lossy().into(),
    };

    let result = ShadowStorage::checkpoint(
        &shadow.repo_path,
        &shadow.session_id,
        &files,
        &removals,
        &meta,
    );

    match result {
        Ok(cp) => {
            debug!(
                session_id = shadow.session_id,
                checkpoint = cp,
                "Shadow checkpoint created"
            );
            shadow.last_event_count = session.events.len();
            shadow.checkpoint_count += 1;
            shadow.last_activity = Instant::now();
        }
        Err(e) => {
            warn!(
                session_id = shadow.session_id,
                "Shadow checkpoint failed: {e}"
            );
        }
    }
}

/// Resolve a file path: if it's absolute, use as-is; otherwise join with project root.
fn resolve_file_path(project_root: &Path, file_path: &str) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(p)
    }
}

/// Condense a shadow branch: archive HAIL + delete shadow.
fn condense_shadow(path: &Path, shadow: &ShadowState) {
    let session = match parse_session(path) {
        Ok(Some(s)) => s,
        Ok(None) => {
            warn!("Cannot condense shadow: no session at {}", path.display());
            return;
        }
        Err(e) => {
            warn!("Cannot condense shadow: parse error: {e}");
            return;
        }
    };

    let hail = match opensession_core::jsonl::to_jsonl_string(&session) {
        Ok(s) => s.into_bytes(),
        Err(e) => {
            warn!("Cannot condense shadow: HAIL serialization failed: {e}");
            return;
        }
    };

    let meta = build_session_meta_json(&session);

    match ShadowStorage::condense(&shadow.repo_path, &shadow.session_id, &hail, &meta) {
        Ok(rel) => info!(
            session_id = shadow.session_id,
            "Condensed shadow → {rel}"
        ),
        Err(e) => warn!(
            session_id = shadow.session_id,
            "Shadow condense failed: {e}"
        ),
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
        session: serde_json::to_value(session)?,
        team_id: Some(config.identity.team_id.clone()),
        body_url: body_url.map(String::from),
        linked_session_ids: None,
        ..Default::default()
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
    fn test_resolve_publish_mode_auto_publish_false_non_manual_becomes_manual() {
        let settings = DaemonSettings {
            auto_publish: false,
            publish_on: PublishMode::Realtime,
            ..Default::default()
        };
        assert_eq!(resolve_publish_mode(&settings), PublishMode::Manual);
    }

    #[test]
    fn test_resolve_file_path_absolute() {
        let root = PathBuf::from("/project");
        let result = resolve_file_path(&root, "/absolute/path/file.rs");
        assert_eq!(result, PathBuf::from("/absolute/path/file.rs"));
    }

    #[test]
    fn test_resolve_file_path_relative() {
        let root = PathBuf::from("/project");
        let result = resolve_file_path(&root, "src/main.rs");
        assert_eq!(result, PathBuf::from("/project/src/main.rs"));
    }
}
