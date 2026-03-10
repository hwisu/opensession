use crate::session_types::SessionSummary;
use serde::{Deserialize, Serialize};

/// Canonical desktop IPC contract version shared between Rust and TS clients.
pub const DESKTOP_IPC_CONTRACT_VERSION: &str = "desktop-ipc-v6";

/// Desktop handoff build request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopHandoffBuildRequest {
    pub session_id: String,
    pub pin_latest: bool,
}

/// Desktop handoff build response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopHandoffBuildResponse {
    pub artifact_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_content: Option<String>,
}

/// Desktop quick-share request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopQuickShareRequest {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// Desktop quick-share response payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopQuickShareResponse {
    pub source_uri: String,
    pub shared_uri: String,
    pub remote: String,
    pub push_cmd: String,
    #[serde(default)]
    pub pushed: bool,
    #[serde(default)]
    pub auto_push_consent: bool,
}

/// Desktop bridge contract/version handshake response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopContractVersionResponse {
    pub version: String,
}

/// Desktop runtime settings payload for App settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSettingsResponse {
    pub session_default_view: String,
    pub summary: DesktopRuntimeSummarySettings,
    pub vector_search: DesktopRuntimeVectorSearchSettings,
    pub change_reader: DesktopRuntimeChangeReaderSettings,
    pub lifecycle: DesktopRuntimeLifecycleSettings,
    pub ui_constraints: DesktopRuntimeSummaryUiConstraints,
}

/// Desktop runtime settings update request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSettingsUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_default_view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<DesktopRuntimeSummarySettingsUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_search: Option<DesktopRuntimeVectorSearchSettingsUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_reader: Option<DesktopRuntimeChangeReaderSettingsUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<DesktopRuntimeLifecycleSettingsUpdate>,
}

/// Local summary provider detection result for desktop setup/settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopSummaryProviderDetectResponse {
    pub detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<DesktopSummaryProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<DesktopSummaryProviderTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryProviderId {
    Disabled,
    Ollama,
    CodexExec,
    ClaudeCli,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryProviderTransport {
    None,
    Cli,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummarySourceMode {
    SessionOnly,
    SessionOrGitChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryResponseStyle {
    Compact,
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryOutputShape {
    Layered,
    FileList,
    SecurityFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryTriggerMode {
    Manual,
    OnSessionSave,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryStorageBackend {
    HiddenRef,
    LocalDb,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryBatchExecutionMode {
    Manual,
    OnAppStart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryBatchScope {
    RecentDays,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryProviderSettings {
    pub id: DesktopSummaryProviderId,
    pub transport: DesktopSummaryProviderTransport,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryPromptSettings {
    pub template: String,
    pub default_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryResponseSettings {
    pub style: DesktopSummaryResponseStyle,
    pub shape: DesktopSummaryOutputShape,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryStorageSettings {
    pub trigger: DesktopSummaryTriggerMode,
    pub backend: DesktopSummaryStorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryBatchSettings {
    pub execution_mode: DesktopSummaryBatchExecutionMode,
    pub scope: DesktopSummaryBatchScope,
    pub recent_days: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummarySettings {
    pub provider: DesktopRuntimeSummaryProviderSettings,
    pub prompt: DesktopRuntimeSummaryPromptSettings,
    pub response: DesktopRuntimeSummaryResponseSettings,
    pub storage: DesktopRuntimeSummaryStorageSettings,
    pub source_mode: DesktopSummarySourceMode,
    pub batch: DesktopRuntimeSummaryBatchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryProviderSettingsUpdate {
    pub id: DesktopSummaryProviderId,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryPromptSettingsUpdate {
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryResponseSettingsUpdate {
    pub style: DesktopSummaryResponseStyle,
    pub shape: DesktopSummaryOutputShape,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryStorageSettingsUpdate {
    pub trigger: DesktopSummaryTriggerMode,
    pub backend: DesktopSummaryStorageBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryBatchSettingsUpdate {
    pub execution_mode: DesktopSummaryBatchExecutionMode,
    pub scope: DesktopSummaryBatchScope,
    pub recent_days: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummarySettingsUpdate {
    pub provider: DesktopRuntimeSummaryProviderSettingsUpdate,
    pub prompt: DesktopRuntimeSummaryPromptSettingsUpdate,
    pub response: DesktopRuntimeSummaryResponseSettingsUpdate,
    pub storage: DesktopRuntimeSummaryStorageSettingsUpdate,
    pub source_mode: DesktopSummarySourceMode,
    pub batch: DesktopRuntimeSummaryBatchSettingsUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeSummaryUiConstraints {
    pub source_mode_locked: bool,
    pub source_mode_locked_value: DesktopSummarySourceMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopVectorSearchProvider {
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopVectorSearchGranularity {
    EventLineChunk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopVectorChunkingMode {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopVectorInstallState {
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopVectorIndexState {
    Idle,
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeVectorSearchSettings {
    pub enabled: bool,
    pub provider: DesktopVectorSearchProvider,
    pub model: String,
    pub endpoint: String,
    pub granularity: DesktopVectorSearchGranularity,
    pub chunking_mode: DesktopVectorChunkingMode,
    pub chunk_size_lines: u16,
    pub chunk_overlap_lines: u16,
    pub top_k_chunks: u16,
    pub top_k_sessions: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeVectorSearchSettingsUpdate {
    pub enabled: bool,
    pub provider: DesktopVectorSearchProvider,
    pub model: String,
    pub endpoint: String,
    pub granularity: DesktopVectorSearchGranularity,
    pub chunking_mode: DesktopVectorChunkingMode,
    pub chunk_size_lines: u16,
    pub chunk_overlap_lines: u16,
    pub top_k_chunks: u16,
    pub top_k_sessions: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopChangeReaderScope {
    SummaryOnly,
    FullContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopChangeReaderVoiceProvider {
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeChangeReaderVoiceSettings {
    pub enabled: bool,
    pub provider: DesktopChangeReaderVoiceProvider,
    pub model: String,
    pub voice: String,
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeChangeReaderVoiceSettingsUpdate {
    pub enabled: bool,
    pub provider: DesktopChangeReaderVoiceProvider,
    pub model: String,
    pub voice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeChangeReaderSettings {
    pub enabled: bool,
    pub scope: DesktopChangeReaderScope,
    pub qa_enabled: bool,
    pub max_context_chars: u32,
    pub voice: DesktopRuntimeChangeReaderVoiceSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeChangeReaderSettingsUpdate {
    pub enabled: bool,
    pub scope: DesktopChangeReaderScope,
    pub qa_enabled: bool,
    pub max_context_chars: u32,
    pub voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeLifecycleSettings {
    pub enabled: bool,
    pub session_ttl_days: u32,
    pub summary_ttl_days: u32,
    pub cleanup_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopRuntimeLifecycleSettingsUpdate {
    pub enabled: bool,
    pub session_ttl_days: u32,
    pub summary_ttl_days: u32,
    pub cleanup_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopLifecycleCleanupState {
    Idle,
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopLifecycleCleanupStatusResponse {
    pub state: DesktopLifecycleCleanupState,
    pub deleted_sessions: u32,
    pub deleted_summaries: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopVectorPreflightResponse {
    pub provider: DesktopVectorSearchProvider,
    pub endpoint: String,
    pub model: String,
    pub ollama_reachable: bool,
    pub model_installed: bool,
    pub install_state: DesktopVectorInstallState,
    pub progress_pct: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopVectorInstallStatusResponse {
    pub state: DesktopVectorInstallState,
    pub model: String,
    pub progress_pct: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopVectorIndexStatusResponse {
    pub state: DesktopVectorIndexState,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum DesktopSummaryBatchState {
    Idle,
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopSummaryBatchStatusResponse {
    pub state: DesktopSummaryBatchState,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    pub failed_sessions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopVectorSessionMatch {
    pub session: SessionSummary,
    pub score: f32,
    pub chunk_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopVectorSearchResponse {
    pub query: String,
    #[serde(default)]
    pub sessions: Vec<DesktopVectorSessionMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub total_candidates: u32,
}

/// Session summary payload returned by desktop runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopSessionSummaryResponse {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub source_details: Option<serde_json::Value>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "any[]"))]
    pub diff_tree: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeReadRequest {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<DesktopChangeReaderScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeReadResponse {
    pub session_id: String,
    pub scope: DesktopChangeReaderScope,
    pub narrative: String,
    #[serde(default)]
    pub citations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<DesktopSummaryProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeQuestionRequest {
    pub session_id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<DesktopChangeReaderScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeReaderTtsRequest {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<DesktopChangeReaderScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeReaderTtsResponse {
    pub mime_type: String,
    pub audio_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopChangeQuestionResponse {
    pub session_id: String,
    pub question: String,
    pub scope: DesktopChangeReaderScope,
    pub answer: String,
    #[serde(default)]
    pub citations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<DesktopSummaryProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
