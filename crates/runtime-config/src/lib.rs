//! Shared runtime configuration types.
//!
//! `opensession-daemon`, desktop runtime, and CLI read/write `opensession.toml`
//! using these types. Runtime-specific logic (watch-path resolution, project
//! config merging, UI/IPC adapters) lives in each runtime crate.

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub vector_search: VectorSearchSettings,
    #[serde(default)]
    pub change_reader: ChangeReaderSettings,
    #[serde(default)]
    pub lifecycle: LifecycleSettings,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummarySettings {
    #[serde(default)]
    pub provider: SummaryProviderSettings,
    #[serde(default)]
    pub prompt: SummaryPromptSettings,
    #[serde(default)]
    pub response: SummaryResponseSettings,
    #[serde(default)]
    pub storage: SummaryStorageSettings,
    /// Kept for CLI/CI and non-desktop runtimes.
    #[serde(default)]
    pub source_mode: SummarySourceMode,
    #[serde(default)]
    pub batch: SummaryBatchSettings,
}

impl SummarySettings {
    pub fn is_configured(&self) -> bool {
        match self.provider.id {
            SummaryProvider::Disabled => false,
            SummaryProvider::Ollama => !self.provider.model.trim().is_empty(),
            SummaryProvider::CodexExec | SummaryProvider::ClaudeCli => true,
        }
    }

    pub fn provider_transport(&self) -> SummaryProviderTransport {
        self.provider.id.transport()
    }

    pub fn allows_git_changes_fallback(&self) -> bool {
        matches!(self.source_mode, SummarySourceMode::SessionOrGitChanges)
    }

    pub fn should_generate_on_session_save(&self) -> bool {
        matches!(self.storage.trigger, SummaryTriggerMode::OnSessionSave)
    }

    pub fn persists_to_local_db(&self) -> bool {
        matches!(self.storage.backend, SummaryStorageBackend::LocalDb)
    }

    pub fn persists_to_hidden_ref(&self) -> bool {
        matches!(self.storage.backend, SummaryStorageBackend::HiddenRef)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryProviderSettings {
    #[serde(default)]
    pub id: SummaryProvider,
    #[serde(default = "default_summary_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
}

impl Default for SummaryProviderSettings {
    fn default() -> Self {
        Self {
            id: SummaryProvider::default(),
            endpoint: default_summary_endpoint(),
            model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryPromptSettings {
    #[serde(default)]
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryResponseSettings {
    #[serde(default)]
    pub style: SummaryResponseStyle,
    #[serde(default)]
    pub shape: SummaryOutputShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SummaryStorageSettings {
    #[serde(default)]
    pub trigger: SummaryTriggerMode,
    #[serde(default)]
    pub backend: SummaryStorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryBatchSettings {
    #[serde(default)]
    pub execution_mode: SummaryBatchExecutionMode,
    #[serde(default)]
    pub scope: SummaryBatchScope,
    #[serde(default = "default_summary_batch_recent_days")]
    pub recent_days: u16,
}

impl Default for SummaryBatchSettings {
    fn default() -> Self {
        Self {
            execution_mode: SummaryBatchExecutionMode::default(),
            scope: SummaryBatchScope::default(),
            recent_days: default_summary_batch_recent_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub provider: VectorSearchProvider,
    #[serde(default = "default_vector_model")]
    pub model: String,
    #[serde(default = "default_vector_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub granularity: VectorSearchGranularity,
    #[serde(default)]
    pub chunking_mode: VectorChunkingMode,
    #[serde(default = "default_vector_chunk_size_lines")]
    pub chunk_size_lines: u16,
    #[serde(default = "default_vector_chunk_overlap_lines")]
    pub chunk_overlap_lines: u16,
    #[serde(default = "default_vector_top_k_chunks")]
    pub top_k_chunks: u16,
    #[serde(default = "default_vector_top_k_sessions")]
    pub top_k_sessions: u16,
}

impl Default for VectorSearchSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            provider: VectorSearchProvider::default(),
            model: default_vector_model(),
            endpoint: default_vector_endpoint(),
            granularity: VectorSearchGranularity::default(),
            chunking_mode: VectorChunkingMode::default(),
            chunk_size_lines: default_vector_chunk_size_lines(),
            chunk_overlap_lines: default_vector_chunk_overlap_lines(),
            top_k_chunks: default_vector_top_k_chunks(),
            top_k_sessions: default_vector_top_k_sessions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeReaderSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub scope: ChangeReaderScope,
    #[serde(default = "default_true")]
    pub qa_enabled: bool,
    #[serde(default = "default_change_reader_max_context_chars")]
    pub max_context_chars: u32,
    #[serde(default)]
    pub voice: ChangeReaderVoiceSettings,
}

impl Default for ChangeReaderSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            scope: ChangeReaderScope::default(),
            qa_enabled: default_true(),
            max_context_chars: default_change_reader_max_context_chars(),
            voice: ChangeReaderVoiceSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeReaderVoiceSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub provider: ChangeReaderVoiceProvider,
    #[serde(default = "default_change_reader_voice_model")]
    pub model: String,
    #[serde(default = "default_change_reader_voice_name")]
    pub voice: String,
    #[serde(default)]
    pub api_key: String,
}

impl Default for ChangeReaderVoiceSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            provider: ChangeReaderVoiceProvider::default(),
            model: default_change_reader_voice_model(),
            voice: default_change_reader_voice_name(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_lifecycle_session_ttl_days")]
    pub session_ttl_days: u32,
    #[serde(default = "default_lifecycle_summary_ttl_days")]
    pub summary_ttl_days: u32,
    #[serde(default = "default_lifecycle_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,
}

impl Default for LifecycleSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            session_ttl_days: default_lifecycle_session_ttl_days(),
            summary_ttl_days: default_lifecycle_summary_ttl_days(),
            cleanup_interval_secs: default_lifecycle_cleanup_interval_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReaderScope {
    #[default]
    SummaryOnly,
    FullContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchProvider {
    #[default]
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchGranularity {
    #[default]
    EventLineChunk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorChunkingMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReaderVoiceProvider {
    #[default]
    Openai,
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

impl SummaryProvider {
    pub fn transport(&self) -> SummaryProviderTransport {
        match self {
            Self::Disabled => SummaryProviderTransport::None,
            Self::Ollama => SummaryProviderTransport::Http,
            Self::CodexExec | Self::ClaudeCli => SummaryProviderTransport::Cli,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryProviderTransport {
    #[default]
    None,
    Cli,
    Http,
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
pub enum SummaryStorageBackend {
    None,
    #[default]
    HiddenRef,
    LocalDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryBatchExecutionMode {
    Manual,
    #[default]
    OnAppStart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SummaryBatchScope {
    #[default]
    RecentDays,
    All,
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
fn default_vector_endpoint() -> String {
    "http://127.0.0.1:11434".to_string()
}
fn default_vector_model() -> String {
    "bge-m3".to_string()
}
fn default_vector_chunk_size_lines() -> u16 {
    12
}
fn default_vector_chunk_overlap_lines() -> u16 {
    3
}
fn default_vector_top_k_chunks() -> u16 {
    30
}
fn default_vector_top_k_sessions() -> u16 {
    20
}
fn default_change_reader_max_context_chars() -> u32 {
    12_000
}
fn default_change_reader_voice_model() -> String {
    "gpt-4o-mini-tts".to_string()
}
fn default_change_reader_voice_name() -> String {
    "alloy".to_string()
}
fn default_summary_batch_recent_days() -> u16 {
    30
}
fn default_lifecycle_session_ttl_days() -> u32 {
    30
}
fn default_lifecycle_summary_ttl_days() -> u32 {
    30
}
fn default_lifecycle_cleanup_interval_secs() -> u64 {
    3_600
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
[summary.provider]
id = "openai"
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
source_mode = "session_or_git_changes"

[summary.provider]
id = "ollama"
endpoint = "http://localhost:11434"
model = "llama3.2:3b"

[summary.prompt]
template = "Use {{HAIL_COMPACT}} only"

[summary.response]
style = "detailed"
shape = "security_first"

[summary.storage]
trigger = "on_session_save"
backend = "local_db"

[summary.batch]
execution_mode = "manual"
scope = "all"
recent_days = 90
"#,
        )
        .expect("parse summary settings");

        assert_eq!(cfg.summary.provider.id, SummaryProvider::Ollama);
        assert_eq!(cfg.summary.provider.endpoint, "http://localhost:11434");
        assert_eq!(cfg.summary.provider.model, "llama3.2:3b");
        assert_eq!(
            cfg.summary.source_mode,
            SummarySourceMode::SessionOrGitChanges
        );
        assert_eq!(cfg.summary.prompt.template, "Use {{HAIL_COMPACT}} only");
        assert_eq!(cfg.summary.response.style, SummaryResponseStyle::Detailed);
        assert_eq!(
            cfg.summary.response.shape,
            SummaryOutputShape::SecurityFirst
        );
        assert_eq!(
            cfg.summary.storage.trigger,
            SummaryTriggerMode::OnSessionSave
        );
        assert_eq!(cfg.summary.storage.backend, SummaryStorageBackend::LocalDb);
        assert_eq!(
            cfg.summary.batch.execution_mode,
            SummaryBatchExecutionMode::Manual
        );
        assert_eq!(cfg.summary.batch.scope, SummaryBatchScope::All);
        assert_eq!(cfg.summary.batch.recent_days, 90);
        assert!(cfg.summary.is_configured());
    }

    #[test]
    fn summary_batch_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert_eq!(
            cfg.summary.batch.execution_mode,
            SummaryBatchExecutionMode::OnAppStart
        );
        assert_eq!(cfg.summary.batch.scope, SummaryBatchScope::RecentDays);
        assert_eq!(cfg.summary.batch.recent_days, 30);
    }

    #[test]
    fn summary_response_style_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.response]
style = "verbose"
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
[summary.response]
shape = "grouped"
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
[summary.storage]
trigger = "always"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary trigger_mode must be rejected"
        );
    }

    #[test]
    fn summary_storage_backend_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.storage]
backend = "remote_db"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary storage.backend must be rejected"
        );
    }

    #[test]
    fn summary_batch_execution_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.batch]
execution_mode = "scheduled"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary batch execution mode must be rejected"
        );
    }

    #[test]
    fn summary_batch_scope_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[summary.batch]
scope = "recent_weeks"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported summary batch scope must be rejected"
        );
    }

    #[test]
    fn summary_provider_accepts_cli_variants() {
        let codex_cfg: DaemonConfig = toml::from_str(
            r#"
[summary.provider]
id = "codex_exec"
"#,
        )
        .expect("parse codex summary provider");
        assert_eq!(codex_cfg.summary.provider.id, SummaryProvider::CodexExec);
        assert!(codex_cfg.summary.is_configured());

        let claude_cfg: DaemonConfig = toml::from_str(
            r#"
[summary.provider]
id = "claude_cli"
"#,
        )
        .expect("parse claude summary provider");
        assert_eq!(claude_cfg.summary.provider.id, SummaryProvider::ClaudeCli);
        assert!(claude_cfg.summary.is_configured());
    }

    #[test]
    fn summary_is_configured_requires_model_only_for_ollama() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.provider.id = SummaryProvider::Ollama;
        cfg.summary.provider.model.clear();
        assert!(!cfg.summary.is_configured());

        cfg.summary.provider.model = "llama3.2:3b".to_string();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider.id = SummaryProvider::CodexExec;
        cfg.summary.provider.model.clear();
        assert!(cfg.summary.is_configured());

        cfg.summary.provider.id = SummaryProvider::ClaudeCli;
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
    fn summary_default_storage_uses_hidden_ref_backend() {
        let cfg = DaemonConfig::default();
        assert_eq!(
            cfg.summary.storage.trigger,
            SummaryTriggerMode::OnSessionSave
        );
        assert_eq!(
            cfg.summary.storage.backend,
            SummaryStorageBackend::HiddenRef
        );
        assert!(cfg.summary.should_generate_on_session_save());
        assert!(cfg.summary.persists_to_hidden_ref());
    }

    #[test]
    fn summary_provider_transport_matches_provider_kind() {
        let mut cfg = DaemonConfig::default();
        cfg.summary.provider.id = SummaryProvider::Disabled;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::None
        );

        cfg.summary.provider.id = SummaryProvider::Ollama;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::Http
        );

        cfg.summary.provider.id = SummaryProvider::CodexExec;
        assert_eq!(
            cfg.summary.provider_transport(),
            SummaryProviderTransport::Cli
        );
    }

    #[test]
    fn vector_search_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.vector_search.enabled);
        assert_eq!(cfg.vector_search.provider, VectorSearchProvider::Ollama);
        assert_eq!(cfg.vector_search.model, "bge-m3");
        assert_eq!(cfg.vector_search.endpoint, "http://127.0.0.1:11434");
        assert_eq!(
            cfg.vector_search.granularity,
            VectorSearchGranularity::EventLineChunk
        );
        assert_eq!(cfg.vector_search.chunking_mode, VectorChunkingMode::Auto);
        assert_eq!(cfg.vector_search.chunk_size_lines, 12);
        assert_eq!(cfg.vector_search.chunk_overlap_lines, 3);
        assert_eq!(cfg.vector_search.top_k_chunks, 30);
        assert_eq!(cfg.vector_search.top_k_sessions, 20);
    }

    #[test]
    fn vector_search_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[vector_search]
enabled = true
provider = "ollama"
model = "bge-m3"
endpoint = "http://localhost:11434"
granularity = "event_line_chunk"
chunking_mode = "manual"
chunk_size_lines = 16
chunk_overlap_lines = 4
top_k_chunks = 60
top_k_sessions = 10
"#,
        )
        .expect("parse vector search settings");

        assert!(cfg.vector_search.enabled);
        assert_eq!(cfg.vector_search.provider, VectorSearchProvider::Ollama);
        assert_eq!(cfg.vector_search.model, "bge-m3");
        assert_eq!(cfg.vector_search.endpoint, "http://localhost:11434");
        assert_eq!(
            cfg.vector_search.granularity,
            VectorSearchGranularity::EventLineChunk
        );
        assert_eq!(cfg.vector_search.chunking_mode, VectorChunkingMode::Manual);
        assert_eq!(cfg.vector_search.chunk_size_lines, 16);
        assert_eq!(cfg.vector_search.chunk_overlap_lines, 4);
        assert_eq!(cfg.vector_search.top_k_chunks, 60);
        assert_eq!(cfg.vector_search.top_k_sessions, 10);
    }

    #[test]
    fn vector_search_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
provider = "openai"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector provider must be rejected"
        );
    }

    #[test]
    fn vector_search_granularity_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
granularity = "session_text"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector granularity must be rejected"
        );
    }

    #[test]
    fn vector_search_chunking_mode_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[vector_search]
chunking_mode = "adaptive"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported vector chunking mode must be rejected"
        );
    }

    #[test]
    fn change_reader_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.change_reader.enabled);
        assert_eq!(cfg.change_reader.scope, ChangeReaderScope::SummaryOnly);
        assert!(cfg.change_reader.qa_enabled);
        assert_eq!(cfg.change_reader.max_context_chars, 12_000);
        assert!(!cfg.change_reader.voice.enabled);
        assert_eq!(
            cfg.change_reader.voice.provider,
            ChangeReaderVoiceProvider::Openai
        );
        assert_eq!(cfg.change_reader.voice.model, "gpt-4o-mini-tts");
        assert_eq!(cfg.change_reader.voice.voice, "alloy");
        assert!(cfg.change_reader.voice.api_key.is_empty());
    }

    #[test]
    fn change_reader_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[change_reader]
enabled = true
scope = "full_context"
qa_enabled = false
max_context_chars = 24000
[change_reader.voice]
enabled = true
provider = "openai"
model = "gpt-4o-mini-tts"
voice = "nova"
api_key = "sk-local"
"#,
        )
        .expect("parse change reader settings");

        assert!(cfg.change_reader.enabled);
        assert_eq!(cfg.change_reader.scope, ChangeReaderScope::FullContext);
        assert!(!cfg.change_reader.qa_enabled);
        assert_eq!(cfg.change_reader.max_context_chars, 24_000);
        assert!(cfg.change_reader.voice.enabled);
        assert_eq!(
            cfg.change_reader.voice.provider,
            ChangeReaderVoiceProvider::Openai
        );
        assert_eq!(cfg.change_reader.voice.model, "gpt-4o-mini-tts");
        assert_eq!(cfg.change_reader.voice.voice, "nova");
        assert_eq!(cfg.change_reader.voice.api_key, "sk-local");
    }

    #[test]
    fn change_reader_scope_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[change_reader]
scope = "full"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported change reader scope must be rejected"
        );
    }

    #[test]
    fn change_reader_voice_provider_requires_canonical_values() {
        let parsed: Result<DaemonConfig, _> = toml::from_str(
            r#"
[change_reader.voice]
provider = "azure"
"#,
        );
        assert!(
            parsed.is_err(),
            "unsupported change reader voice provider must be rejected"
        );
    }

    #[test]
    fn lifecycle_defaults_are_stable() {
        let cfg = DaemonConfig::default();
        assert!(cfg.lifecycle.enabled);
        assert_eq!(cfg.lifecycle.session_ttl_days, 30);
        assert_eq!(cfg.lifecycle.summary_ttl_days, 30);
        assert_eq!(cfg.lifecycle.cleanup_interval_secs, 3_600);
    }

    #[test]
    fn lifecycle_settings_deserialize_from_toml() {
        let cfg: DaemonConfig = toml::from_str(
            r#"
[lifecycle]
enabled = true
session_ttl_days = 45
summary_ttl_days = 14
cleanup_interval_secs = 7200
"#,
        )
        .expect("parse lifecycle settings");

        assert!(cfg.lifecycle.enabled);
        assert_eq!(cfg.lifecycle.session_ttl_days, 45);
        assert_eq!(cfg.lifecycle.summary_ttl_days, 14);
        assert_eq!(cfg.lifecycle.cleanup_interval_secs, 7_200);
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
