use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_core::sanitize::{sanitize_session, SanitizeConfig};
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use opensession_parsers::{all_parsers, SessionParser};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{debug, error, info, warn};

use crate::config::{DaemonConfig, PublishMode};
use crate::retry::retry_upload;
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

    // Warn if deprecated auto_publish is set to false while publish_on is not Manual
    if !config.daemon.auto_publish && config.daemon.publish_on != PublishMode::Manual {
        warn!(
            "auto_publish=false is deprecated, treating as publish_on=manual. \
             Please update your config to use publish_on = \"manual\" instead."
        );
    }

    // Resolve effective publish mode
    let effective_mode = if !config.daemon.auto_publish {
        &PublishMode::Manual
    } else {
        &config.daemon.publish_on
    };

    // Pending changes: path -> when we last saw a change
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            // Receive new file change events
            Some(event) = rx.recv() => {
                debug!("Scheduling: {:?}", event.path.display());
                match effective_mode {
                    PublishMode::Realtime => {
                        pending.insert(event.path, Instant::now());
                    }
                    _ => {
                        pending.insert(event.path, Instant::now());
                    }
                }
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

/// Process a single file: parse, store in local DB, sanitize, upload
async fn process_file(path: &PathBuf, config: &DaemonConfig, db: &LocalDb) -> Result<()> {
    // Check if file was already uploaded since last modification
    let modified: DateTime<Utc> = std::fs::metadata(path)?.modified()?.into();

    let path_str = path.to_string_lossy().to_string();
    if db.was_uploaded_after(&path_str, &modified)? {
        debug!("Skipping already-uploaded file: {}", path.display());
        return Ok(());
    }

    // Find a parser
    let parsers = all_parsers();
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(path))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => {
            warn!("No parser for: {}", path.display());
            return Ok(());
        }
    };

    info!("Parsing: {} ({})", path.display(), parser.name());
    let mut session = parser.parse(path)?;

    // exclude_tools filter: skip if this session's tool is excluded
    if config
        .privacy
        .exclude_tools
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&session.agent.tool))
    {
        info!(
            "Excluding tool '{}': {}",
            session.agent.tool,
            path.display()
        );
        return Ok(());
    }

    // Extract git context from the session's working directory
    let cwd = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str().map(String::from));
    let git = cwd.as_deref().map(extract_git_context).unwrap_or_default();

    // Store in local DB (before sanitization, so we keep git context)
    db.upsert_local_session(&session, &path_str, &git)?;

    // Sanitize
    let sanitize_config = SanitizeConfig {
        strip_paths: config.privacy.strip_paths,
        strip_env_vars: config.privacy.strip_env_vars,
        exclude_patterns: config.privacy.exclude_patterns.clone(),
    };
    sanitize_session(&mut session, &sanitize_config);

    // Upload with retry
    let url = format!("{}/api/sessions", config.server.url.trim_end_matches('/'));
    info!("Uploading session {} to {}", session.session_id, url);

    let upload_body = serde_json::json!({
        "session": session,
        "team_id": config.identity.team_id,
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    let response = retry_upload(
        &client,
        &url,
        &config.server.api_key,
        &upload_body,
        config.daemon.max_retries,
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
