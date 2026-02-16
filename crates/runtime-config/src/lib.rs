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
    /// Enable timeline/event summary generation in TUI detail view.
    #[serde(default = "default_false")]
    pub summary_enabled: bool,
    /// Summary engine/provider selection.
    /// Examples: `anthropic`, `openai`, `openai-compatible`, `gemini`, `cli:auto`, `cli:codex`.
    #[serde(default)]
    pub summary_provider: Option<String>,
    /// Optional model override for summary generation.
    #[serde(default)]
    pub summary_model: Option<String>,
    /// Summary verbosity mode (`normal` or `minimal`).
    #[serde(default = "default_summary_content_mode")]
    pub summary_content_mode: String,
    /// Persist summary cache entries on disk/local DB.
    #[serde(default = "default_true")]
    pub summary_disk_cache_enabled: bool,
    /// OpenAI-compatible endpoint full URL override.
    #[serde(default)]
    pub summary_openai_compat_endpoint: Option<String>,
    /// OpenAI-compatible base URL override.
    #[serde(default)]
    pub summary_openai_compat_base: Option<String>,
    /// OpenAI-compatible path override.
    #[serde(default)]
    pub summary_openai_compat_path: Option<String>,
    /// OpenAI-compatible payload style (`chat` or `responses`).
    #[serde(default)]
    pub summary_openai_compat_style: Option<String>,
    /// OpenAI-compatible API key.
    #[serde(default)]
    pub summary_openai_compat_key: Option<String>,
    /// OpenAI-compatible API key header (default `Authorization` when omitted).
    #[serde(default)]
    pub summary_openai_compat_key_header: Option<String>,
    /// Number of events to include in each summary window.
    /// `0` means adaptive mode.
    #[serde(default = "default_summary_event_window")]
    pub summary_event_window: u32,
    /// Debounce before dispatching summary jobs.
    #[serde(default = "default_summary_debounce_ms")]
    pub summary_debounce_ms: u64,
    /// Max concurrent summary jobs.
    #[serde(default = "default_summary_max_inflight")]
    pub summary_max_inflight: u32,
    /// Internal one-way migration marker for summary window v2 semantics.
    #[serde(default = "default_false")]
    pub summary_window_migrated_v2: bool,
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
            summary_enabled: false,
            summary_provider: None,
            summary_model: None,
            summary_content_mode: default_summary_content_mode(),
            summary_disk_cache_enabled: true,
            summary_openai_compat_endpoint: None,
            summary_openai_compat_base: None,
            summary_openai_compat_path: None,
            summary_openai_compat_style: None,
            summary_openai_compat_key: None,
            summary_openai_compat_key_header: None,
            summary_event_window: default_summary_event_window(),
            summary_debounce_ms: default_summary_debounce_ms(),
            summary_max_inflight: default_summary_max_inflight(),
            summary_window_migrated_v2: false,
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
fn default_detail_auto_expand_selected_event() -> bool {
    true
}
fn default_publish_on() -> PublishMode {
    PublishMode::Manual
}
fn default_summary_content_mode() -> String {
    "normal".to_string()
}
fn default_summary_event_window() -> u32 {
    0
}
fn default_summary_debounce_ms() -> u64 {
    250
}
fn default_summary_max_inflight() -> u32 {
    2
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

    #[test]
    fn daemon_summary_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(cfg.daemon.detail_auto_expand_selected_event);
        assert!(!cfg.daemon.summary_enabled);
        assert_eq!(cfg.daemon.summary_provider, None);
        assert_eq!(cfg.daemon.summary_model, None);
        assert_eq!(cfg.daemon.summary_content_mode, "normal");
        assert!(cfg.daemon.summary_disk_cache_enabled);
        assert_eq!(cfg.daemon.summary_event_window, 0);
        assert_eq!(cfg.daemon.summary_debounce_ms, 250);
        assert_eq!(cfg.daemon.summary_max_inflight, 2);
        assert!(!cfg.daemon.summary_window_migrated_v2);
    }

    #[test]
    fn daemon_summary_fields_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[daemon]
summary_enabled = true
summary_provider = "openai"
summary_model = "gpt-4o-mini"
summary_content_mode = "minimal"
summary_disk_cache_enabled = false
summary_event_window = 8
summary_debounce_ms = 100
summary_max_inflight = 4
summary_window_migrated_v2 = false
detail_auto_expand_selected_event = false
"#,
        )
        .expect("parse summary config");

        assert!(!cfg.daemon.detail_auto_expand_selected_event);
        assert!(cfg.daemon.summary_enabled);
        assert_eq!(cfg.daemon.summary_provider.as_deref(), Some("openai"));
        assert_eq!(cfg.daemon.summary_model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(cfg.daemon.summary_content_mode, "minimal");
        assert!(!cfg.daemon.summary_disk_cache_enabled);
        assert_eq!(cfg.daemon.summary_event_window, 8);
        assert_eq!(cfg.daemon.summary_debounce_ms, 100);
        assert_eq!(cfg.daemon.summary_max_inflight, 4);
        assert!(!cfg.daemon.summary_window_migrated_v2);
    }
}
