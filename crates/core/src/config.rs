//! Shared daemon/TUI configuration types.
//!
//! Both `opensession-daemon` and `opensession-tui` read/write `daemon.toml`
//! using these types. Daemon-specific logic (watch-path resolution, project
//! config merging) lives in the daemon crate; TUI-specific logic (settings
//! layout, field editing) lives in the TUI crate.

use serde::{Deserialize, Serialize};

/// Top-level daemon configuration (persisted as `daemon.toml`).
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
    /// Neglect-live tool rules (stream-write/PostToolUse): matching sessions skip detail live and summary.
    #[serde(default)]
    pub stream_write: Vec<String>,
    /// Enable timeline summaries in TUI detail view.
    #[serde(default = "default_summary_enabled")]
    pub summary_enabled: bool,
    /// Summary provider override:
    /// auto | anthropic | openai | openai-compatible | gemini | cli:auto | cli:codex | cli:claude | cli:cursor | cli:gemini
    #[serde(default)]
    pub summary_provider: Option<String>,
    /// Optional model override for summary calls (API and CLI `--model`).
    #[serde(default)]
    pub summary_model: Option<String>,
    /// Summary detail mode: normal | minimal.
    #[serde(default = "default_summary_content_mode")]
    pub summary_content_mode: String,
    /// Persist timeline summaries to disk and reuse by context hash.
    #[serde(default = "default_summary_disk_cache_enabled")]
    pub summary_disk_cache_enabled: bool,
    /// Full OpenAI-compatible endpoint URL override.
    #[serde(default)]
    pub summary_openai_compat_endpoint: Option<String>,
    /// OpenAI-compatible base URL (used when endpoint is not set).
    #[serde(default)]
    pub summary_openai_compat_base: Option<String>,
    /// OpenAI-compatible path (default: /chat/completions).
    #[serde(default)]
    pub summary_openai_compat_path: Option<String>,
    /// OpenAI-compatible payload style: chat | responses.
    #[serde(default)]
    pub summary_openai_compat_style: Option<String>,
    /// Optional OpenAI-compatible API key.
    #[serde(default)]
    pub summary_openai_compat_key: Option<String>,
    /// Optional API key header name (default: Authorization: Bearer).
    #[serde(default)]
    pub summary_openai_compat_key_header: Option<String>,
    /// Number of events per summary window. `0` means auto(turn-aware).
    #[serde(default = "default_summary_event_window")]
    pub summary_event_window: u32,
    /// One-shot migration guard for legacy summary window defaults.
    #[serde(default = "default_false")]
    pub summary_window_migrated_v2: bool,
    /// Debounce for summary requests / realtime checks, in milliseconds.
    #[serde(default = "default_summary_debounce_ms")]
    pub summary_debounce_ms: u64,
    /// Max concurrent in-flight timeline summary jobs.
    #[serde(default = "default_summary_max_inflight")]
    pub summary_max_inflight: u32,
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
            stream_write: Vec::new(),
            summary_enabled: true,
            summary_provider: None,
            summary_model: None,
            summary_content_mode: "normal".to_string(),
            summary_disk_cache_enabled: true,
            summary_openai_compat_endpoint: None,
            summary_openai_compat_base: None,
            summary_openai_compat_path: None,
            summary_openai_compat_style: None,
            summary_openai_compat_key: None,
            summary_openai_compat_key_header: None,
            summary_event_window: 0,
            summary_window_migrated_v2: false,
            summary_debounce_ms: 1200,
            summary_max_inflight: 1,
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
    #[serde(default = "default_true")]
    pub claude_code: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
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
            cursor: false,
            custom_paths: Vec::new(),
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
fn default_summary_enabled() -> bool {
    true
}
fn default_summary_content_mode() -> String {
    "normal".to_string()
}
fn default_summary_disk_cache_enabled() -> bool {
    true
}
fn default_summary_event_window() -> u32 {
    0
}
fn default_summary_debounce_ms() -> u64 {
    1200
}
fn default_summary_max_inflight() -> u32 {
    1
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
