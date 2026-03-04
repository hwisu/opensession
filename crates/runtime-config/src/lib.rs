//! Shared runtime configuration types.
//!
//! `opensession-daemon`, desktop runtime, and CLI read/write `opensession.toml`
//! using these types. Runtime-specific logic (watch-path resolution, project
//! config merging, UI/IPC adapters) lives in each runtime crate.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Canonical config file name used by daemon/desktop/cli.
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
    #[serde(default)]
    pub summary: SummarySettings,
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
    /// Default detail view mode for session timeline rendering.
    #[serde(default = "default_session_default_view")]
    pub session_default_view: SessionDefaultView,
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
            session_default_view: SessionDefaultView::default(),
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionDefaultView {
    #[default]
    Full,
    Compressed,
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
pub struct SummarySettings {
    #[serde(default)]
    pub provider: SummaryProvider,
    #[serde(default = "default_summary_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub source_mode: SummarySourceMode,
    #[serde(default)]
    pub response_style: SummaryResponseStyle,
    #[serde(default)]
    pub output_shape: SummaryOutputShape,
    #[serde(default)]
    pub output_instruction: String,
    #[serde(default)]
    pub trigger_mode: SummaryTriggerMode,
    #[serde(default)]
    pub persist_mode: SummaryPersistMode,
    #[serde(default)]
    pub template_slots: BTreeMap<String, String>,
}

impl Default for SummarySettings {
    fn default() -> Self {
        Self {
            provider: SummaryProvider::default(),
            endpoint: default_summary_endpoint(),
            model: String::new(),
            source_mode: SummarySourceMode::default(),
            response_style: SummaryResponseStyle::default(),
            output_shape: SummaryOutputShape::default(),
            output_instruction: String::new(),
            trigger_mode: SummaryTriggerMode::default(),
            persist_mode: SummaryPersistMode::default(),
            template_slots: BTreeMap::new(),
        }
    }
}

impl SummarySettings {
    pub fn is_configured(&self) -> bool {
        match self.provider {
            SummaryProvider::Disabled => false,
            SummaryProvider::Ollama => !self.model.trim().is_empty(),
            SummaryProvider::CodexExec | SummaryProvider::ClaudeCli => true,
        }
    }

    pub fn allows_git_changes_fallback(&self) -> bool {
        matches!(self.source_mode, SummarySourceMode::SessionOrGitChanges)
    }

    pub fn should_generate_on_session_save(&self) -> bool {
        matches!(self.trigger_mode, SummaryTriggerMode::OnSessionSave)
    }

    pub fn persists_to_local_db(&self) -> bool {
        matches!(self.persist_mode, SummaryPersistMode::LocalDb)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryProvider {
    #[default]
    Disabled,
    Ollama,
    CodexExec,
    ClaudeCli,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryResponseStyle {
    Compact,
    #[default]
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummarySourceMode {
    #[default]
    SessionOnly,
    SessionOrGitChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryOutputShape {
    #[default]
    Layered,
    FileList,
    SecurityFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryTriggerMode {
    Manual,
    #[default]
    OnSessionSave,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryPersistMode {
    None,
    #[default]
    LocalDb,
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
    /// Store sessions as git objects on hidden refs (git-native).
    #[default]
    Native,
    /// Store session bodies in SQLite-backed storage.
    Sqlite,
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
fn default_session_default_view() -> SessionDefaultView {
    SessionDefaultView::Full
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
fn default_summary_endpoint() -> String {
    "http://127.0.0.1:11434".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_storage_method_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[git_storage]
method = "platform_api"
"#,
        );
        assert!(parsed.is_err(), "legacy aliases must be rejected");
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

    #[test]
    fn summary_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
provider = "openai"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary provider must be rejected"
        );
    }

    #[test]
    fn summary_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[summary]
provider = "ollama"
endpoint = "http://localhost:11434"
model = "llama3.2:3b"
source_mode = "session_or_git_changes"
response_style = "detailed"
output_shape = "security_first"
output_instruction = "Call out risky auth delta first."
trigger_mode = "on_session_save"
persist_mode = "local_db"

[summary.template_slots]
changes = "focus on modifications"
auth_security = "security-first"
"#,
        )
        .expect("parse summary settings");

        assert_eq!(cfg.summary.provider, SummaryProvider::Ollama);
        assert_eq!(cfg.summary.endpoint, "http://localhost:11434");
        assert_eq!(cfg.summary.model, "llama3.2:3b");
        assert_eq!(
            cfg.summary.source_mode,
            SummarySourceMode::SessionOrGitChanges
        );
        assert_eq!(cfg.summary.response_style, SummaryResponseStyle::Detailed);
        assert_eq!(cfg.summary.output_shape, SummaryOutputShape::SecurityFirst);
        assert_eq!(
            cfg.summary.output_instruction,
            "Call out risky auth delta first."
        );
        assert_eq!(cfg.summary.trigger_mode, SummaryTriggerMode::OnSessionSave);
        assert_eq!(cfg.summary.persist_mode, SummaryPersistMode::LocalDb);
        assert_eq!(
            cfg.summary
                .template_slots
                .get("changes")
                .map(String::as_str),
            Some("focus on modifications")
        );
        assert!(cfg.summary.is_configured());
    }

    #[test]
    fn summary_response_style_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
response_style = "verbose"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary response_style must be rejected"
        );
    }

    #[test]
    fn summary_source_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
source_mode = "git_only"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary source_mode must be rejected"
        );
    }

    #[test]
    fn summary_output_shape_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
output_shape = "grouped"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary output_shape must be rejected"
        );
    }

    #[test]
    fn summary_trigger_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
trigger_mode = "always"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary trigger_mode must be rejected"
        );
    }

    #[test]
    fn summary_persist_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary]
persist_mode = "remote_db"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary persist_mode must be rejected"
        );
    }

    #[test]
    fn summary_provider_accepts_cli_variants() {
        let codex_cfg: DaemonConfig = toml::from_str(
            r#"
[summary]
provider = "codex_exec"
"#,
        )
        .expect("parse codex summary provider");
        assert_eq!(codex_cfg.summary.provider, SummaryProvider::CodexExec);
        assert!(codex_cfg.summary.is_configured());

        let claude_cfg: DaemonConfig = toml::from_str(
            r#"
[summary]
provider = "claude_cli"
"#,
        )
        .expect("parse claude summary provider");
        assert_eq!(claude_cfg.summary.provider, SummaryProvider::ClaudeCli);
        assert!(claude_cfg.summary.is_configured());
    }

    #[test]
    fn summary_is_configured_requires_model_only_for_ollama() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.provider = SummaryProvider::Ollama;
        cfg.summary.model.clear();
        assert!(!cfg.summary.is_configured());

        cfg.summary.model = "llama3.2:3b".to_string();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider = SummaryProvider::CodexExec;
        cfg.summary.model.clear();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider = SummaryProvider::ClaudeCli;
        assert!(cfg.summary.is_configured());
    }

    #[test]
    fn summary_git_fallback_availability_depends_on_source_mode() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.source_mode = SummarySourceMode::SessionOnly;
        assert!(!cfg.summary.allows_git_changes_fallback());

        cfg.summary.source_mode = SummarySourceMode::SessionOrGitChanges;
        assert!(cfg.summary.allows_git_changes_fallback());
    }

    #[test]
    fn summary_default_trigger_and_persist_modes_are_automatic_local() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.summary.trigger_mode, SummaryTriggerMode::OnSessionSave);
        assert_eq!(cfg.summary.persist_mode, SummaryPersistMode::LocalDb);
        assert!(cfg.summary.should_generate_on_session_save());
        assert!(cfg.summary.persists_to_local_db());
    }

    #[test]
    fn daemon_default_session_view_deserializes_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[daemon]
session_default_view = "compressed"
"#,
        )
        .expect("parse daemon session_default_view");

        assert_eq!(
            cfg.daemon.session_default_view,
            SessionDefaultView::Compressed
        );
    }

    #[test]
    fn daemon_default_session_view_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[daemon]
session_default_view = "compact"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported session_default_view must fail"
        );
    }
}
