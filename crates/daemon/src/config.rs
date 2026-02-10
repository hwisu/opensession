use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    #[serde(default)]
    pub daemon: DaemonSettings,
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub identity: IdentitySettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
    #[serde(default)]
    pub watchers: WatcherSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    #[serde(default = "default_true")]
    pub auto_publish: bool,
    #[serde(default = "default_debounce")]
    pub debounce_secs: u64,
    #[serde(default = "default_publish_on")]
    pub publish_on: PublishMode,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_secs: u64,
    #[serde(default = "default_realtime_debounce_ms")]
    pub realtime_debounce_ms: u64,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            auto_publish: true,
            debounce_secs: 5,
            publish_on: PublishMode::SessionEnd,
            max_retries: 3,
            health_check_interval_secs: 300,
            realtime_debounce_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PublishMode {
    SessionEnd,
    Realtime,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_server_url")]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            url: default_server_url(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySettings {
    #[serde(default = "default_nickname")]
    pub nickname: String,
    /// Team ID to upload sessions to
    #[serde(default)]
    pub team_id: String,
}

impl Default for IdentitySettings {
    fn default() -> Self {
        Self {
            nickname: default_nickname(),
            team_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub strip_paths: bool,
    #[serde(default = "default_true")]
    pub strip_env_vars: bool,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            strip_paths: true,
            strip_env_vars: true,
            exclude_patterns: default_exclude_patterns(),
            exclude_tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherSettings {
    #[serde(default = "default_true")]
    pub claude_code: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub goose: bool,
    #[serde(default = "default_true")]
    pub aider: bool,
    #[serde(default)]
    pub cursor: bool,
    #[serde(default)]
    pub custom_paths: Vec<String>,
}

impl Default for WatcherSettings {
    fn default() -> Self {
        Self {
            claude_code: true,
            opencode: true,
            goose: true,
            aider: true,
            cursor: false,
            custom_paths: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_debounce() -> u64 {
    5
}

fn default_max_retries() -> u32 {
    3
}

fn default_health_check_interval() -> u64 {
    300
}

fn default_realtime_debounce_ms() -> u64 {
    500
}

fn default_publish_on() -> PublishMode {
    PublishMode::SessionEnd
}

fn default_server_url() -> String {
    "https://opensession.io".to_string()
}

fn default_nickname() -> String {
    "user".to_string()
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "*.env".to_string(),
        "*secret*".to_string(),
        "*credential*".to_string(),
    ]
}

/// Get the config directory path
pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".config").join("opensession"))
}

/// Get the daemon config file path
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.toml"))
}

/// Get the PID file path
pub fn pid_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.pid"))
}

/// Get the state file path
pub fn state_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("state.json"))
}

/// Load daemon config from disk
pub fn load_config() -> Result<DaemonConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(DaemonConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read daemon config at {}", path.display()))?;
    let config: DaemonConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse daemon config at {}", path.display()))?;
    Ok(config)
}

/// Resolve watch paths based on watcher config
pub fn resolve_watch_paths(config: &DaemonConfig) -> Vec<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    let mut paths = Vec::new();

    if config.watchers.claude_code {
        let p = home.join(".claude").join("projects");
        if p.exists() {
            paths.push(p);
        }
    }

    if config.watchers.opencode {
        let p = home.join(".local").join("share").join("opencode");
        if p.exists() {
            paths.push(p);
        }
    }

    if config.watchers.goose {
        let p = home.join(".local").join("share").join("goose");
        if p.exists() {
            paths.push(p);
        }
    }

    if config.watchers.aider {
        let p = home.join(".aider");
        if p.exists() {
            paths.push(p);
        }
    }

    if config.watchers.cursor {
        let p = home.join(".cursor");
        if p.exists() {
            paths.push(p);
        }
    }

    for custom in &config.watchers.custom_paths {
        let p = PathBuf::from(shellexpand(custom, &home));
        if p.exists() {
            paths.push(p);
        }
    }

    paths
}

/// Simple ~ expansion
fn shellexpand(path: &str, home: &Path) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", home.display(), rest)
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serializes() {
        let config = DaemonConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("auto_publish = true"));
        assert!(toml_str.contains("debounce_secs = 5"));
        assert!(toml_str.contains("publish_on = \"session_end\""));
        assert!(toml_str.contains("max_retries = 3"));
        assert!(toml_str.contains("health_check_interval_secs = 300"));
        assert!(toml_str.contains("realtime_debounce_ms = 500"));
    }

    #[test]
    fn test_config_roundtrip() {
        let config = DaemonConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: DaemonConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.daemon.debounce_secs, 5);
        assert_eq!(parsed.daemon.publish_on, PublishMode::SessionEnd);
        assert_eq!(parsed.daemon.max_retries, 3);
        assert_eq!(parsed.daemon.health_check_interval_secs, 300);
        assert_eq!(parsed.daemon.realtime_debounce_ms, 500);
        assert!(parsed.watchers.claude_code);
        assert!(!parsed.watchers.cursor);
    }
}
