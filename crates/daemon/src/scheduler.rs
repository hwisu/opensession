use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::sanitize::{SanitizeConfig, sanitize_session};
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

/// Tracks which sessions have been uploaded
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UploadState {
    /// Map of file path -> last uploaded timestamp
    pub uploaded: HashMap<String, DateTime<Utc>>,
    /// Map of file path -> byte offset for incremental reads (Phase 3)
    #[serde(default)]
    pub offsets: HashMap<String, u64>,
}

impl UploadState {
    pub fn load(path: &PathBuf) -> Self {
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write state file at {}", path.display()))?;
        Ok(())
    }

    pub fn mark_uploaded(&mut self, file_path: &str) {
        self.uploaded.insert(file_path.to_string(), Utc::now());
    }

    pub fn was_uploaded_after(&self, file_path: &str, modified: DateTime<Utc>) -> bool {
        self.uploaded
            .get(file_path)
            .map(|uploaded_at| *uploaded_at >= modified)
            .unwrap_or(false)
    }
}

/// Run the scheduler loop: receives file change events, debounces, parses, and uploads.
pub async fn run_scheduler(
    config: DaemonConfig,
    mut rx: mpsc::UnboundedReceiver<FileChangeEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let debounce_duration = Duration::from_secs(config.daemon.debounce_secs);
    let state_path = crate::config::state_file_path().unwrap_or_else(|_| PathBuf::from("state.json"));
    let mut state = UploadState::load(&state_path);

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
                        // In realtime mode, process with minimal debounce
                        // Phase 3 will replace this with incremental streaming
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
                            if let Err(e) = process_file(&path, &config, &mut state, &state_path).await {
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

    // Save state on shutdown
    if let Err(e) = state.save(&state_path) {
        error!("Failed to save state on shutdown: {}", e);
    }
}

/// Process a single file: parse, sanitize, upload
async fn process_file(
    path: &PathBuf,
    config: &DaemonConfig,
    state: &mut UploadState,
    state_path: &PathBuf,
) -> Result<()> {
    // Check if file was already uploaded since last modification
    let modified: DateTime<Utc> = std::fs::metadata(path)?
        .modified()?
        .into();

    let path_str = path.to_string_lossy().to_string();
    if state.was_uploaded_after(&path_str, modified) {
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
        state.mark_uploaded(&path_str);
        state.save(state_path)?;
    } else if status.is_client_error() {
        let body = response.text().await.unwrap_or_default();
        error!("Upload rejected (HTTP {}): {}", status, body);
    } else {
        let body = response.text().await.unwrap_or_default();
        error!("Upload failed (HTTP {}): {}", status, body);
    }

    Ok(())
}
