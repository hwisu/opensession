//! Shared daemon/TUI configuration types.
//!
//! Both `opensession-daemon` and `opensession-tui` read/write `opensession.toml`
//! using these types. Daemon-specific logic (watch-path resolution, project
//! config merging) lives in the daemon crate; TUI-specific logic (settings
//! layout, field editing) lives in the TUI crate.

use serde::{Deserialize, Serialize};

/// Canonical config file name used by daemon/cli/tui.
pub const CONFIG_FILE_NAME: &str = "opensession.toml";

/// Top-level daemon configuration (persisted as `opensession.toml`).
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
    #[serde(default)]
    pub git_storage: GitStorageSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSettings {
    #[serde(default = "default_false")]
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
    /// Enable realtime file preview refresh in TUI session detail.
    #[serde(default = "default_detail_realtime_preview_enabled")]
    pub detail_realtime_preview_enabled: bool,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            auto_publish: false,
            debounce_secs: 5,
            publish_on: PublishMode::Manual,
            max_retries: 3,
            health_check_interval_secs: 300,
            realtime_debounce_ms: 500,
            detail_realtime_preview_enabled: false,
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

impl PublishMode {
    pub fn cycle(&self) -> Self {
        match self {
            Self::SessionEnd => Self::Realtime,
            Self::Realtime => Self::Manual,
            Self::Manual => Self::SessionEnd,
        }
    }

    pub fn display(&self) -> &'static str {
        match self {
            Self::SessionEnd => "Session End",
            Self::Realtime => "Realtime",
            Self::Manual => "Manual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalendarDisplayMode {
    Smart,
    Relative,
    Absolute,
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
    /// Deprecated agent toggles kept for backward-compatible config parsing.
    #[serde(default = "default_true", skip_serializing)]
    pub claude_code: bool,
    /// Deprecated agent toggles kept for backward-compatible config parsing.
    #[serde(default = "default_true", skip_serializing)]
    pub opencode: bool,
    /// Deprecated agent toggles kept for backward-compatible config parsing.
    #[serde(default = "default_true", skip_serializing)]
    pub cursor: bool,
    #[serde(default = "default_watch_paths")]
    pub custom_paths: Vec<String>,
}

impl Default for WatcherSettings {
    fn default() -> Self {
        Self {
            claude_code: true,
            opencode: true,
            cursor: true,
            custom_paths: default_watch_paths(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStorageSettings {
    #[serde(default)]
    pub method: GitStorageMethod,
    #[serde(default)]
    pub token: String,
}

impl Default for GitStorageSettings {
    fn default() -> Self {
        Self {
            method: GitStorageMethod::Native,
            token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitStorageMethod {
    PlatformApi,
    /// Store sessions as git objects on an orphan branch (no external API needed).
    Native,
    #[default]
    #[serde(other)]
    None,
}

// ── Serde default functions ─────────────────────────────────────────────

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
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
fn default_detail_realtime_preview_enabled() -> bool {
    false
}
fn default_publish_on() -> PublishMode {
    PublishMode::Manual
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

pub const DEFAULT_WATCH_PATHS: &[&str] = &[
    "~/.claude/projects",
    "~/.codex/sessions",
    "~/.local/share/opencode/storage/session",
    "~/.cline/data/tasks",
    "~/.local/share/amp/threads",
    "~/.gemini/tmp",
    "~/Library/Application Support/Cursor/User",
    "~/.config/Cursor/User",
];

pub fn default_watch_paths() -> Vec<String> {
    DEFAULT_WATCH_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect()
}

/// Apply compatibility fallbacks after loading raw TOML.
/// Returns true when any field was updated.
pub fn apply_compat_fallbacks(config: &mut DaemonConfig, root: Option<&toml::Value>) -> bool {
    let mut changed = false;

    if config_file_missing_git_storage_method(root)
        && config.git_storage.method == GitStorageMethod::None
    {
        config.git_storage.method = GitStorageMethod::Native;
        changed = true;
    }

    if config.identity.team_id.trim().is_empty() {
        if let Some(team_id) = root
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get("server"))
            .and_then(toml::Value::as_table)
            .and_then(|section| section.get("team_id"))
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            config.identity.team_id = team_id.to_string();
            changed = true;
        }
    }

    if config.watchers.custom_paths.is_empty() {
        config.watchers.custom_paths = default_watch_paths();
        changed = true;
    }

    changed
}

/// True when `[git_storage].method` is absent/invalid in the source TOML.
pub fn config_file_missing_git_storage_method(root: Option<&toml::Value>) -> bool {
    let Some(root) = root else {
        return false;
    };
    let Some(table) = root.as_table() else {
        return false;
    };
    let Some(git_storage) = table.get("git_storage") else {
        return true;
    };
    match git_storage.as_table() {
        Some(section) => !section.contains_key("method"),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_compat_fallbacks_populates_legacy_fields() {
        let mut cfg = DaemonConfig::default();
        cfg.git_storage.method = GitStorageMethod::None;
        cfg.identity.team_id.clear();
        cfg.watchers.custom_paths.clear();

        let root: toml::Value = toml::from_str(
            r#"
[server]
team_id = "team-legacy"

[git_storage]
"#,
        )
        .expect("parse toml");

        let changed = apply_compat_fallbacks(&mut cfg, Some(&root));
        assert!(changed);
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Native);
        assert_eq!(cfg.identity.team_id, "team-legacy");
        assert!(!cfg.watchers.custom_paths.is_empty());
    }

    #[test]
    fn apply_compat_fallbacks_is_noop_for_modern_values() {
        let mut cfg = DaemonConfig::default();
        cfg.identity.team_id = "team-modern".to_string();
        cfg.watchers.custom_paths = vec!["/tmp/one".to_string()];

        let root: toml::Value = toml::from_str(
            r#"
[server]
team_id = "team-from-file"

[git_storage]
method = "native"
"#,
        )
        .expect("parse toml");

        let before = cfg.clone();
        let changed = apply_compat_fallbacks(&mut cfg, Some(&root));
        assert!(!changed);
        assert_eq!(cfg.identity.team_id, before.identity.team_id);
        assert_eq!(cfg.watchers.custom_paths, before.watchers.custom_paths);
        assert_eq!(cfg.git_storage.method, before.git_storage.method);
    }

    #[test]
    fn legacy_watcher_flags_are_not_serialized() {
        let cfg = DaemonConfig::default();
        let encoded = toml::to_string(&cfg).expect("serialize config");

        assert!(encoded.contains("custom_paths"));
        assert!(!encoded.contains("\nclaude_code ="));
        assert!(!encoded.contains("\nopencode ="));
        assert!(!encoded.contains("\ncursor ="));
    }
}
