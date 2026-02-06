use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use opensession_core::sanitize::{SanitizeConfig, sanitize_session};
use opensession_parsers::{all_parsers, SessionParser};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::config::DaemonConfig;
use crate::watcher::FileChangeEvent;

/// Tracks which sessions have been uploaded
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UploadState {
    /// Map of file path -> last uploaded timestamp
    pub uploaded: HashMap<String, DateTime<Utc>>,
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

    // Pending changes: path -> when we last saw a change
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

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
                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, last_change)| now.duration_since(**last_change) >= debounce_duration)
                    .map(|(path, _)| path.clone())
                    .collect();

                for path in ready {
                    pending.remove(&path);
                    if config.daemon.auto_publish {
                        if let Err(e) = process_file(&path, &config, &mut state, &state_path).await {
                            error!("Failed to process {}: {:#}", path.display(), e);
                        }
                    } else {
                        debug!("Auto-publish disabled, skipping: {}", path.display());
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

    // Sanitize
    let sanitize_config = SanitizeConfig {
        strip_paths: config.privacy.strip_paths,
        strip_env_vars: config.privacy.strip_env_vars,
        exclude_patterns: config.privacy.exclude_patterns.clone(),
    };
    sanitize_session(&mut session, &sanitize_config);

    // Upload
    let url = format!("{}/api/sessions", config.server.url.trim_end_matches('/'));
    info!("Uploading session {} to {}", session.session_id, url);

    let upload_body = serde_json::json!({
        "session": session,
        "visibility": "public"
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&upload_body)
        .send()
        .await
        .context("Failed to connect to server")?;

    let status = response.status();
    if status.is_success() {
        info!("Uploaded session: {}", session.session_id);
        state.mark_uploaded(&path_str);
        state.save(state_path)?;
    } else {
        let body = response.text().await.unwrap_or_default();
        error!("Upload failed (HTTP {}): {}", status, body);
    }

    Ok(())
}
