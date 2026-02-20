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
    /// Expand selected timeline event detail rows by default in TUI session detail.
    #[serde(default = "default_detail_auto_expand_selected_event")]
    pub detail_auto_expand_selected_event: bool,
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
            detail_auto_expand_selected_event: true,
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
}

impl Default for IdentitySettings {
    fn default() -> Self {
        Self {
            nickname: default_nickname(),
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
    #[serde(default = "default_watch_paths")]
    pub custom_paths: Vec<String>,
}

impl Default for WatcherSettings {
    fn default() -> Self {
        Self {
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
    #[serde(default)]
    pub retention: GitRetentionSettings,
}

impl Default for GitStorageSettings {
    fn default() -> Self {
        Self {
            method: GitStorageMethod::Native,
            token: String::new(),
            retention: GitRetentionSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRetentionSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default = "default_git_retention_keep_days")]
    pub keep_days: u32,
    #[serde(default = "default_git_retention_interval_secs")]
    pub interval_secs: u64,
}

impl Default for GitRetentionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_days: default_git_retention_keep_days(),
            interval_secs: default_git_retention_interval_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitStorageMethod {
    /// Store sessions as git objects on an orphan branch (branch-based git-native).
    #[default]
    #[serde(alias = "platform_api", alias = "platform-api", alias = "api")]
    Native,
    /// Store session bodies in SQLite-backed storage.
    #[serde(alias = "none", alias = "sqlite_local", alias = "sqlite-local")]
    Sqlite,
    /// Unknown/invalid values are normalized by compatibility fallbacks.
    #[serde(other)]
    Unknown,
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
fn default_detail_auto_expand_selected_event() -> bool {
    true
}
fn default_publish_on() -> PublishMode {
    PublishMode::Manual
}
fn default_git_retention_keep_days() -> u32 {
    30
}
fn default_git_retention_interval_secs() -> u64 {
    86_400
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
pub fn apply_compat_fallbacks(config: &mut DaemonConfig, _root: Option<&toml::Value>) -> bool {
    let mut changed = false;

    if config.git_storage.method == GitStorageMethod::Unknown {
        config.git_storage.method = GitStorageMethod::Native;
        changed = true;
    }

    if config.watchers.custom_paths.is_empty() {
        config.watchers.custom_paths = default_watch_paths();
        changed = true;
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_compat_fallbacks_populates_missing_fields() {
        let mut cfg = DaemonConfig::default();
        cfg.git_storage.method = GitStorageMethod::Unknown;
        cfg.watchers.custom_paths.clear();

        let root: toml::Value = toml::from_str(
            r#"
[git_storage]
"#,
        )
        .expect("parse toml");

        let changed = apply_compat_fallbacks(&mut cfg, Some(&root));
        assert!(changed);
        assert_eq!(cfg.git_storage.method, GitStorageMethod::Native);
        assert!(!cfg.watchers.custom_paths.is_empty());
    }

    #[test]
    fn git_storage_method_compat_aliases_are_accepted() {
        let compat_none: DaemonConfig = toml::from_str(
            r#"
[git_storage]
method = "none"
"#,
        )
        .expect("parse toml");
        assert_eq!(compat_none.git_storage.method, GitStorageMethod::Sqlite);

        let compat_platform_api: DaemonConfig = toml::from_str(
            r#"
[git_storage]
method = "platform_api"
"#,
        )
        .expect("parse toml");
        assert_eq!(
            compat_platform_api.git_storage.method,
            GitStorageMethod::Native
        );
    }

    #[test]
    fn apply_compat_fallbacks_is_noop_for_modern_values() {
        let mut cfg = DaemonConfig::default();
        cfg.watchers.custom_paths = vec!["/tmp/one".to_string()];

        let root: toml::Value = toml::from_str(
            r#"
[git_storage]
method = "native"
"#,
        )
        .expect("parse toml");

        let before = cfg.clone();
        let changed = apply_compat_fallbacks(&mut cfg, Some(&root));
        assert!(!changed);
        assert_eq!(cfg.watchers.custom_paths, before.watchers.custom_paths);
        assert_eq!(cfg.git_storage.method, before.git_storage.method);
    }

    #[test]
    fn unknown_watcher_flags_are_ignored() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[watchers]
claude_code = false
opencode = false
cursor = false
custom_paths = ["~/.codex/sessions"]
"#,
        )
        .expect("parse watcher config");

        assert_eq!(
            cfg.watchers.custom_paths,
            vec!["~/.codex/sessions".to_string()]
        );
    }

    #[test]
    fn watcher_settings_serialize_only_current_fields() {
        let cfg = DaemonConfig::default();
        let encoded = toml::to_string(&cfg).expect("serialize config");

        assert!(encoded.contains("custom_paths"));
        assert!(!encoded.contains("\nclaude_code ="));
        assert!(!encoded.contains("\nopencode ="));
        assert!(!encoded.contains("\ncursor ="));
    }

    #[test]
    fn git_retention_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.git_storage.retention.enabled);
        assert_eq!(cfg.git_storage.retention.keep_days, 30);
        assert_eq!(cfg.git_storage.retention.interval_secs, 86_400);
    }

    #[test]
    fn git_retention_fields_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[git_storage]
method = "native"

[git_storage.retention]
enabled = true
keep_days = 14
interval_secs = 43200
"#,
        )
        .expect("parse retention config");

        assert_eq!(cfg.git_storage.method, GitStorageMethod::Native);
        assert!(cfg.git_storage.retention.enabled);
        assert_eq!(cfg.git_storage.retention.keep_days, 14);
        assert_eq!(cfg.git_storage.retention.interval_secs, 43_200);
    }
}
