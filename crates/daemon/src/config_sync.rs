use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::config::DaemonConfig;

/// Server-managed configuration, synced periodically
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncedConfig {
    #[serde(default)]
    pub privacy: Option<SyncedPrivacy>,
    #[serde(default)]
    pub watchers: Option<SyncedWatchers>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_synced: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedPrivacy {
    pub exclude_patterns: Option<Vec<String>>,
    pub exclude_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedWatchers {
    pub claude_code: Option<bool>,
    pub opencode: Option<bool>,
    pub goose: Option<bool>,
    pub aider: Option<bool>,
    pub cursor: Option<bool>,
}

/// Get synced config file path
fn synced_config_path() -> Result<PathBuf> {
    Ok(crate::config::config_dir()?.join("synced.toml"))
}

/// Load synced config from disk
pub fn load_synced_config() -> SyncedConfig {
    let path = match synced_config_path() {
        Ok(p) => p,
        Err(_) => return SyncedConfig::default(),
    };

    if !path.exists() {
        return SyncedConfig::default();
    }

    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save synced config to disk
fn save_synced_config(config: &SyncedConfig) -> Result<()> {
    let path = synced_config_path()?;
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let content = toml::to_string_pretty(config).context("Failed to serialize synced config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write synced config at {}", path.display()))?;
    Ok(())
}

/// Merge local + synced configs. Local config takes priority.
/// For exclude_patterns and exclude_tools, use union (deduplicated).
pub fn merge_configs(local: &DaemonConfig, synced: &SyncedConfig) -> DaemonConfig {
    let mut merged = local.clone();

    if let Some(ref sp) = synced.privacy {
        // Union for exclude_patterns
        if let Some(ref patterns) = sp.exclude_patterns {
            let mut combined: Vec<String> = merged.privacy.exclude_patterns.clone();
            for p in patterns {
                if !combined.contains(p) {
                    combined.push(p.clone());
                }
            }
            merged.privacy.exclude_patterns = combined;
        }

        // Union for exclude_tools
        if let Some(ref tools) = sp.exclude_tools {
            let mut combined: Vec<String> = merged.privacy.exclude_tools.clone();
            for t in tools {
                if !combined.contains(t) {
                    combined.push(t.clone());
                }
            }
            merged.privacy.exclude_tools = combined;
        }
    }

    if let Some(ref sw) = synced.watchers {
        // Synced watchers only apply if local hasn't explicitly set them
        // Since we can't distinguish "explicitly set to true" from default,
        // synced watchers only disable (set to false), never enable
        if let Some(false) = sw.claude_code {
            merged.watchers.claude_code = false;
        }
        if let Some(false) = sw.opencode {
            merged.watchers.opencode = false;
        }
        if let Some(false) = sw.goose {
            merged.watchers.goose = false;
        }
        if let Some(false) = sw.aider {
            merged.watchers.aider = false;
        }
        if let Some(true) = sw.cursor {
            merged.watchers.cursor = true;
        }
    }

    merged
}

/// Run the config sync poller.
/// Polls the server every `interval_secs` for config updates.
pub async fn run_config_sync(
    server_url: String,
    api_key: String,
    interval_secs: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    if interval_secs == 0 {
        info!("Config sync disabled (interval=0)");
        return;
    }

    // Use same interval as health check by default (5 min)
    let poll_interval = Duration::from_secs(interval_secs);
    let mut interval = tokio::time::interval(poll_interval);
    // Skip first tick
    interval.tick().await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let mut current_etag: Option<String> = load_synced_config().etag;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                match poll_config(&client, &server_url, &api_key, &current_etag).await {
                    Ok(PollResult::Updated(config, new_etag)) => {
                        info!("Config sync: received updated config from server");
                        current_etag = new_etag.clone();
                        let synced = SyncedConfig {
                            privacy: config.privacy.map(|p| SyncedPrivacy {
                                exclude_patterns: p.exclude_patterns,
                                exclude_tools: p.exclude_tools,
                            }),
                            watchers: config.watchers.map(|w| SyncedWatchers {
                                claude_code: w.claude_code,
                                opencode: w.opencode,
                                goose: w.goose,
                                aider: w.aider,
                                cursor: w.cursor,
                            }),
                            etag: new_etag,
                            last_synced: Some(Utc::now()),
                        };
                        if let Err(e) = save_synced_config(&synced) {
                            warn!("Failed to save synced config: {}", e);
                        }
                    }
                    Ok(PollResult::NotModified) => {
                        debug!("Config sync: no changes (304)");
                    }
                    Ok(PollResult::Unavailable) => {
                        debug!("Config sync: server unavailable, using cached config");
                    }
                    Err(e) => {
                        warn!("Config sync error: {}", e);
                    }
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    debug!("Config sync shutting down");
                    break;
                }
            }
        }
    }
}

enum PollResult {
    Updated(ConfigSyncData, Option<String>),
    NotModified,
    Unavailable,
}

#[derive(Deserialize)]
struct ConfigSyncData {
    privacy: Option<SyncedPrivacyData>,
    watchers: Option<SyncedWatchersData>,
}

#[derive(Deserialize)]
struct SyncedPrivacyData {
    exclude_patterns: Option<Vec<String>>,
    exclude_tools: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct SyncedWatchersData {
    claude_code: Option<bool>,
    opencode: Option<bool>,
    goose: Option<bool>,
    aider: Option<bool>,
    cursor: Option<bool>,
}

async fn poll_config(
    client: &reqwest::Client,
    server_url: &str,
    api_key: &str,
    current_etag: &Option<String>,
) -> Result<PollResult> {
    let url = format!("{}/api/config/sync", server_url.trim_end_matches('/'));

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }
    if let Some(etag) = current_etag {
        req = req.header("If-None-Match", etag.as_str());
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => return Ok(PollResult::Unavailable),
    };

    match resp.status().as_u16() {
        200 => {
            let new_etag = resp
                .headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let data: ConfigSyncData = resp
                .json()
                .await
                .context("Failed to parse config sync response")?;
            Ok(PollResult::Updated(data, new_etag))
        }
        304 => Ok(PollResult::NotModified),
        404 => {
            // Server doesn't support config sync yet
            debug!("Config sync endpoint not found (404), skipping");
            Ok(PollResult::Unavailable)
        }
        _ => Ok(PollResult::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DaemonConfig, PrivacySettings};

    #[test]
    fn test_merge_configs_union_patterns() {
        let local = DaemonConfig {
            privacy: PrivacySettings {
                exclude_patterns: vec!["*.env".to_string(), "*secret*".to_string()],
                exclude_tools: vec!["cursor".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        let synced = SyncedConfig {
            privacy: Some(SyncedPrivacy {
                exclude_patterns: Some(vec!["*secret*".to_string(), "*token*".to_string()]),
                exclude_tools: Some(vec!["aider".to_string(), "cursor".to_string()]),
            }),
            ..Default::default()
        };

        let merged = merge_configs(&local, &synced);

        // Union, no duplicates
        assert_eq!(
            merged.privacy.exclude_patterns,
            vec!["*.env", "*secret*", "*token*"]
        );
        assert_eq!(merged.privacy.exclude_tools, vec!["cursor", "aider"]);
    }

    #[test]
    fn test_merge_configs_local_priority() {
        let local = DaemonConfig {
            privacy: PrivacySettings {
                strip_paths: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let synced = SyncedConfig::default();
        let merged = merge_configs(&local, &synced);

        // Local override preserved
        assert!(!merged.privacy.strip_paths);
    }

    #[test]
    fn test_merge_configs_empty_synced() {
        let local = DaemonConfig::default();
        let synced = SyncedConfig::default();
        let merged = merge_configs(&local, &synced);

        assert_eq!(
            merged.privacy.exclude_patterns,
            local.privacy.exclude_patterns
        );
    }
}
