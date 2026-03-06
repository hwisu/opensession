//! Shared API types, crypto, and SQL builders for opensession.io
//!
//! This crate is the **single source of truth** for all API request/response types.
//! TypeScript types are auto-generated via `ts-rs` and consumed by the frontend.
//!
//! To regenerate TypeScript types:
//!   cargo test -p opensession-api -- export_typescript --nocapture

use serde::{Deserialize, Serialize};

#[cfg(feature = "backend")]
pub mod crypto;
#[cfg(feature = "backend")]
pub mod db;
pub mod deploy;
pub mod oauth;
#[cfg(feature = "backend")]
pub mod service;

// Re-export core HAIL types for convenience
pub use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext, Stats,
};

// ─── Shared Enums ────────────────────────────────────────────────────────────

/// Sort order for session listings.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum SortOrder {
    #[default]
    Recent,
    Popular,
    Longest,
}

impl SortOrder {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Recent => "recent",
            Self::Popular => "popular",
            Self::Longest => "longest",
        }
    }
}

impl std::fmt::Display for SortOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Time range filter for queries.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum TimeRange {
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
    #[serde(rename = "30d")]
    Days30,
    #[default]
    #[serde(rename = "all")]
    All,
}

impl TimeRange {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
            Self::All => "all",
        }
    }
}

impl std::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Type of link between two sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum LinkType {
    Handoff,
    Related,
    Parent,
    Child,
}

impl LinkType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Handoff => "handoff",
            Self::Related => "related",
            Self::Parent => "parent",
            Self::Child => "child",
        }
    }
}

impl std::fmt::Display for LinkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Utilities ───────────────────────────────────────────────────────────────

/// Safely convert `u64` to `i64`, saturating at `i64::MAX` instead of wrapping.
pub fn saturating_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

// ─── Auth ────────────────────────────────────────────────────────────────────

/// Email + password registration.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthRegisterRequest {
    pub email: String,
    pub password: String,
    pub nickname: String,
}

/// Email + password login.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Returned on successful login / register / refresh.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub user_id: String,
    pub nickname: String,
}

/// Refresh token request.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Logout request (invalidate refresh token).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// Change password request.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Returned by `POST /api/auth/verify` — confirms token validity.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct VerifyResponse {
    pub user_id: String,
    pub nickname: String,
}

/// Full user profile returned by `GET /api/auth/me`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UserSettingsResponse {
    pub user_id: String,
    pub nickname: String,
    pub created_at: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    /// Linked OAuth providers.
    #[serde(default)]
    pub oauth_providers: Vec<oauth::LinkedProvider>,
}

/// Generic success response for operations that don't return data.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OkResponse {
    pub ok: bool,
}

/// Response for API key issuance. The key is visible only at issuance time.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct IssueApiKeyResponse {
    pub api_key: String,
}

/// Public metadata for a user-managed git credential.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct GitCredentialSummary {
    pub id: String,
    pub label: String,
    pub host: String,
    pub path_prefix: String,
    pub header_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

/// Response for `GET /api/auth/git-credentials`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListGitCredentialsResponse {
    #[serde(default)]
    pub credentials: Vec<GitCredentialSummary>,
}

/// Request for `POST /api/auth/git-credentials`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateGitCredentialRequest {
    pub label: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    pub header_name: String,
    pub header_value: String,
}

/// Response for OAuth link initiation (redirect URL).
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OAuthLinkResponse {
    pub url: String,
}

// ─── Sessions ────────────────────────────────────────────────────────────────

/// Request body for `POST /api/sessions` — upload a recorded session.
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadRequest {
    pub session: Session,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_session_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_repo_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_plugin: Option<String>,
}

/// Returned on successful session upload — contains the new session ID and URL.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UploadResponse {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub session_score: i64,
    #[serde(default = "default_score_plugin")]
    pub score_plugin: String,
}

/// Flat session summary returned by list/detail endpoints.
/// This is NOT the full HAIL Session — it's a DB-derived summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionSummary {
    pub id: String,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub tool: String,
    pub agent_provider: Option<String>,
    pub agent_model: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Comma-separated tags string
    pub tags: Option<String>,
    pub created_at: String,
    pub uploaded_at: String,
    pub message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_repo_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_read: Option<String>,
    #[serde(default)]
    pub has_errors: bool,
    #[serde(default = "default_max_active_agents")]
    pub max_active_agents: i64,
    #[serde(default)]
    pub session_score: i64,
    #[serde(default = "default_score_plugin")]
    pub score_plugin: String,
}

/// Paginated session listing returned by `GET /api/sessions`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

/// Canonical desktop IPC contract version shared between Rust and TS clients.
pub const DESKTOP_IPC_CONTRACT_VERSION: &str = "desktop-ipc-v6";

/// Query parameters for `GET /api/sessions` — pagination, filtering, sorting.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    pub search: Option<String>,
    pub tool: Option<String>,
    pub git_repo_name: Option<String>,
    /// Sort order (default: recent)
    pub sort: Option<SortOrder>,
    /// Time range filter (default: all)
    pub time_range: Option<TimeRange>,
}

/// Desktop session list query payload passed through Tauri invoke.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopSessionListQuery {
    pub page: Option<String>,
    pub per_page: Option<String>,
    pub search: Option<String>,
    pub tool: Option<String>,
    pub git_repo_name: Option<String>,
    pub sort: Option<String>,
    pub time_range: Option<String>,
    pub force_refresh: Option<bool>,
}

/// Repo list response used by server/worker/desktop adapters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionRepoListResponse {
    pub repos: Vec<String>,
}

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

/// Structured desktop bridge error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DesktopApiError {
    pub code: String,
    pub status: u16,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "Record<string, any> | null"))]
    pub details: Option<serde_json::Value>,
}

impl SessionListQuery {
    /// Returns true when this query targets the anonymous public feed and is safe to edge-cache.
    pub fn is_public_feed_cacheable(
        &self,
        has_auth_header: bool,
        has_session_cookie: bool,
    ) -> bool {
        !has_auth_header
            && !has_session_cookie
            && self.search.as_deref().is_none_or(|s| s.trim().is_empty())
            && self
                .git_repo_name
                .as_deref()
                .is_none_or(|repo| repo.trim().is_empty())
            && self.page <= 10
            && self.per_page <= 50
    }
}

#[cfg(test)]
mod session_list_query_tests {
    use super::*;

    fn base_query() -> SessionListQuery {
        SessionListQuery {
            page: 1,
            per_page: 20,
            search: None,
            tool: None,
            git_repo_name: None,
            sort: None,
            time_range: None,
        }
    }

    #[test]
    fn public_feed_cacheable_when_anonymous_default_feed() {
        let q = base_query();
        assert!(q.is_public_feed_cacheable(false, false));
    }

    #[test]
    fn public_feed_not_cacheable_with_auth_or_cookie() {
        let q = base_query();
        assert!(!q.is_public_feed_cacheable(true, false));
        assert!(!q.is_public_feed_cacheable(false, true));
    }

    #[test]
    fn public_feed_not_cacheable_for_search_or_large_page() {
        let mut q = base_query();
        q.search = Some("hello".into());
        assert!(!q.is_public_feed_cacheable(false, false));

        let mut q = base_query();
        q.git_repo_name = Some("org/repo".into());
        assert!(!q.is_public_feed_cacheable(false, false));

        let mut q = base_query();
        q.page = 11;
        assert!(!q.is_public_feed_cacheable(false, false));

        let mut q = base_query();
        q.per_page = 100;
        assert!(!q.is_public_feed_cacheable(false, false));
    }
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}
fn default_max_active_agents() -> i64 {
    1
}

fn default_score_plugin() -> String {
    opensession_core::scoring::DEFAULT_SCORE_PLUGIN.to_string()
}

/// Single session detail returned by `GET /api/sessions/:id`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionDetail {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub summary: SessionSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_sessions: Vec<SessionLink>,
}

/// A link between two sessions (e.g., handoff chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionLink {
    pub session_id: String,
    pub linked_session_id: String,
    pub link_type: LinkType,
    pub created_at: String,
}

/// Source descriptor for parser preview requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum ParseSource {
    /// Fetch and parse a raw file from a generic Git remote/ref/path source.
    Git {
        remote: String,
        r#ref: String,
        path: String,
    },
    /// Fetch and parse a raw file from a public GitHub repository.
    Github {
        owner: String,
        repo: String,
        r#ref: String,
        path: String,
    },
    /// Parse inline file content supplied by clients (for local upload preview).
    Inline {
        filename: String,
        /// Base64-encoded UTF-8 text content.
        content_base64: String,
    },
}

/// Candidate parser ranked by detection confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParseCandidate {
    pub id: String,
    pub confidence: u8,
    pub reason: String,
}

/// Request body for `POST /api/parse/preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewRequest {
    pub source: ParseSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser_hint: Option<String>,
}

/// Response body for `POST /api/parse/preview`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewResponse {
    pub parser_used: String,
    #[serde(default)]
    pub parser_candidates: Vec<ParseCandidate>,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub session: Session,
    pub source: ParseSource,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_adapter: Option<String>,
}

/// Structured parser preview error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ParsePreviewErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parser_candidates: Vec<ParseCandidate>,
}

/// Local review bundle generated from a PR range.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewBundle {
    pub review_id: String,
    pub generated_at: String,
    pub pr: LocalReviewPrMeta,
    #[serde(default)]
    pub commits: Vec<LocalReviewCommit>,
    #[serde(default)]
    pub sessions: Vec<LocalReviewSession>,
}

/// PR metadata for a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewPrMeta {
    pub url: String,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub remote: String,
    pub base_sha: String,
    pub head_sha: String,
}

/// Reviewer-focused digest extracted from mapped sessions for a commit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewReviewerQa {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// Reviewer-focused digest extracted from mapped sessions for a commit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewReviewerDigest {
    #[serde(default)]
    pub qa: Vec<LocalReviewReviewerQa>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub test_files: Vec<String>,
}

/// Commit row in a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewCommit {
    pub sha: String,
    pub title: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: String,
    #[serde(default)]
    pub session_ids: Vec<String>,
    #[serde(default)]
    pub reviewer_digest: LocalReviewReviewerDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_summary: Option<LocalReviewSemanticSummary>,
}

/// Layer/file summary section for local review semantic payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewLayerFileChange {
    pub layer: String,
    pub summary: String,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Commit-level semantic summary used when session mappings are weak or absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewSemanticSummary {
    pub changes: String,
    pub auth_security: String,
    #[serde(default)]
    pub layer_file_changes: Vec<LocalReviewLayerFileChange>,
    pub source_kind: String,
    pub generation_kind: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(type = "any[]"))]
    pub diff_tree: Vec<serde_json::Value>,
}

/// Session payload mapped into a local review bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LocalReviewSession {
    pub session_id: String,
    pub ledger_ref: String,
    pub hail_path: String,
    #[serde(default)]
    pub commit_shas: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub session: Session,
}

// ─── Streaming Events ────────────────────────────────────────────────────────

/// Request body for `POST /api/sessions/:id/events` — append live events.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct StreamEventsRequest {
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub agent: Option<Agent>,
    #[cfg_attr(feature = "ts", ts(type = "any"))]
    pub context: Option<SessionContext>,
    #[cfg_attr(feature = "ts", ts(type = "any[]"))]
    pub events: Vec<Event>,
}

/// Returned by `POST /api/sessions/:id/events` — number of events accepted.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct StreamEventsResponse {
    pub accepted: usize,
}

// ─── Health ──────────────────────────────────────────────────────────────────

/// Returned by `GET /api/health` — server liveness check.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Returned by `GET /api/capabilities` — runtime feature availability.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CapabilitiesResponse {
    pub auth_enabled: bool,
    pub parse_preview_enabled: bool,
    pub register_targets: Vec<String>,
    pub share_modes: Vec<String>,
}

pub const DEFAULT_REGISTER_TARGETS: &[&str] = &["local", "git"];
pub const DEFAULT_SHARE_MODES: &[&str] = &["web", "git", "quick", "json"];

impl CapabilitiesResponse {
    /// Build runtime capability payload with shared defaults.
    pub fn for_runtime(auth_enabled: bool, parse_preview_enabled: bool) -> Self {
        Self {
            auth_enabled,
            parse_preview_enabled,
            register_targets: DEFAULT_REGISTER_TARGETS
                .iter()
                .map(|target| (*target).to_string())
                .collect(),
            share_modes: DEFAULT_SHARE_MODES
                .iter()
                .map(|mode| (*mode).to_string())
                .collect(),
        }
    }
}

// ─── Service Error ───────────────────────────────────────────────────────────

/// Framework-agnostic service error.
///
/// Each variant maps to an HTTP status code. Both the Axum server and
/// Cloudflare Worker convert this into the appropriate response type.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ServiceError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl ServiceError {
    /// HTTP status code as a `u16`.
    pub fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::Internal(_) => 500,
        }
    }

    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Internal(_) => "internal",
        }
    }

    /// The error message.
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::Internal(m) => m,
        }
    }

    /// Build a closure that logs a DB/IO error and returns `Internal`.
    pub fn from_db<E: std::fmt::Display>(context: &str) -> impl FnOnce(E) -> Self + '_ {
        move |e| Self::Internal(format!("{context}: {e}"))
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ServiceError {}

// ─── Error ───────────────────────────────────────────────────────────────────

/// API error payload.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl From<&ServiceError> for ApiError {
    fn from(e: &ServiceError) -> Self {
        Self {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }
    }
}

// ─── TypeScript generation ───────────────────────────────────────────────────

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn parse_preview_request_round_trip_git() {
        let req = ParsePreviewRequest {
            source: ParseSource::Git {
                remote: "https://github.com/hwisu/opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            parser_hint: Some("hail".to_string()),
        };

        let json = serde_json::to_string(&req).expect("request should serialize");
        let decoded: ParsePreviewRequest =
            serde_json::from_str(&json).expect("request should deserialize");

        match decoded.source {
            ParseSource::Git {
                remote,
                r#ref,
                path,
            } => {
                assert_eq!(remote, "https://github.com/hwisu/opensession");
                assert_eq!(r#ref, "main");
                assert_eq!(path, "sessions/demo.hail.jsonl");
            }
            _ => panic!("expected git parse source"),
        }
        assert_eq!(decoded.parser_hint.as_deref(), Some("hail"));
    }

    #[test]
    fn parse_preview_request_round_trip_github_compat() {
        let req = ParsePreviewRequest {
            source: ParseSource::Github {
                owner: "hwisu".to_string(),
                repo: "opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            parser_hint: Some("hail".to_string()),
        };

        let json = serde_json::to_string(&req).expect("request should serialize");
        let decoded: ParsePreviewRequest =
            serde_json::from_str(&json).expect("request should deserialize");

        match decoded.source {
            ParseSource::Github {
                owner,
                repo,
                r#ref,
                path,
            } => {
                assert_eq!(owner, "hwisu");
                assert_eq!(repo, "opensession");
                assert_eq!(r#ref, "main");
                assert_eq!(path, "sessions/demo.hail.jsonl");
            }
            _ => panic!("expected github parse source"),
        }
        assert_eq!(decoded.parser_hint.as_deref(), Some("hail"));
    }

    #[test]
    fn parse_preview_error_response_round_trip_with_candidates() {
        let payload = ParsePreviewErrorResponse {
            code: "parser_selection_required".to_string(),
            message: "choose parser".to_string(),
            parser_candidates: vec![ParseCandidate {
                id: "codex".to_string(),
                confidence: 89,
                reason: "event markers".to_string(),
            }],
        };

        let json = serde_json::to_string(&payload).expect("error payload should serialize");
        let decoded: ParsePreviewErrorResponse =
            serde_json::from_str(&json).expect("error payload should deserialize");

        assert_eq!(decoded.code, "parser_selection_required");
        assert_eq!(decoded.parser_candidates.len(), 1);
        assert_eq!(decoded.parser_candidates[0].id, "codex");
    }

    #[test]
    fn local_review_bundle_round_trip() {
        let mut sample_session = Session::new(
            "s-review-1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        sample_session.recompute_stats();

        let payload = LocalReviewBundle {
            review_id: "gh-org-repo-pr1-abc1234".to_string(),
            generated_at: "2026-02-24T00:00:00Z".to_string(),
            pr: LocalReviewPrMeta {
                url: "https://github.com/org/repo/pull/1".to_string(),
                owner: "org".to_string(),
                repo: "repo".to_string(),
                number: 1,
                remote: "origin".to_string(),
                base_sha: "a".repeat(40),
                head_sha: "b".repeat(40),
            },
            commits: vec![LocalReviewCommit {
                sha: "c".repeat(40),
                title: "feat: add review flow".to_string(),
                author_name: "Alice".to_string(),
                author_email: "alice@example.com".to_string(),
                authored_at: "2026-02-24T00:00:00Z".to_string(),
                session_ids: vec!["s-review-1".to_string()],
                reviewer_digest: LocalReviewReviewerDigest {
                    qa: vec![LocalReviewReviewerQa {
                        question: "Which route should we verify first?".to_string(),
                        answer: Some("Start with /review/local/:id live path.".to_string()),
                    }],
                    modified_files: vec![
                        "crates/cli/src/review.rs".to_string(),
                        "web/src/routes/review/local/[id]/+page.svelte".to_string(),
                    ],
                    test_files: vec!["web/e2e-live/live-review-local.spec.ts".to_string()],
                },
                semantic_summary: Some(LocalReviewSemanticSummary {
                    changes: "Updated review flow wiring".to_string(),
                    auth_security: "none detected".to_string(),
                    layer_file_changes: vec![LocalReviewLayerFileChange {
                        layer: "application".to_string(),
                        summary: "Added bundle resolver".to_string(),
                        files: vec!["crates/cli/src/review.rs".to_string()],
                    }],
                    source_kind: "git_commit".to_string(),
                    generation_kind: "heuristic_fallback".to_string(),
                    provider: "disabled".to_string(),
                    model: None,
                    error: None,
                    diff_tree: Vec::new(),
                }),
            }],
            sessions: vec![LocalReviewSession {
                session_id: "s-review-1".to_string(),
                ledger_ref: "refs/remotes/origin/opensession/branches/bWFpbg".to_string(),
                hail_path: "v1/se/s-review-1.hail.jsonl".to_string(),
                commit_shas: vec!["c".repeat(40)],
                session: sample_session,
            }],
        };

        let json = serde_json::to_string(&payload).expect("review bundle should serialize");
        let decoded: LocalReviewBundle =
            serde_json::from_str(&json).expect("review bundle should deserialize");

        assert_eq!(decoded.review_id, "gh-org-repo-pr1-abc1234");
        assert_eq!(decoded.pr.number, 1);
        assert_eq!(decoded.commits.len(), 1);
        assert_eq!(decoded.sessions.len(), 1);
        assert_eq!(decoded.sessions[0].session_id, "s-review-1");
        assert_eq!(
            decoded.commits[0]
                .reviewer_digest
                .qa
                .first()
                .map(|row| row.question.as_str()),
            Some("Which route should we verify first?")
        );
        assert_eq!(decoded.commits[0].reviewer_digest.test_files.len(), 1);
    }

    #[test]
    fn capabilities_response_round_trip_includes_new_fields() {
        let caps = CapabilitiesResponse::for_runtime(true, true);

        let json = serde_json::to_string(&caps).expect("capabilities should serialize");
        let decoded: CapabilitiesResponse =
            serde_json::from_str(&json).expect("capabilities should deserialize");

        assert!(decoded.auth_enabled);
        assert!(decoded.parse_preview_enabled);
        assert_eq!(decoded.register_targets, vec!["local", "git"]);
        assert_eq!(decoded.share_modes, vec!["web", "git", "quick", "json"]);
    }

    #[test]
    fn capabilities_defaults_are_stable() {
        assert_eq!(DEFAULT_REGISTER_TARGETS, &["local", "git"]);
        assert_eq!(DEFAULT_SHARE_MODES, &["web", "git", "quick", "json"]);
    }
}

#[cfg(all(test, feature = "ts"))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use ts_rs::TS;

    /// Run with: cargo test -p opensession-api -- export_typescript --nocapture
    #[test]
    fn export_typescript() {
        let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/ui/src/api-types.generated.ts");

        let cfg = ts_rs::Config::new().with_large_int("number");
        let mut parts: Vec<String> = Vec::new();
        parts.push("// AUTO-GENERATED by opensession-api — DO NOT EDIT".to_string());
        parts.push(
            "// Regenerate with: cargo test -p opensession-api -- export_typescript".to_string(),
        );
        parts.push(String::new());

        // Collect all type declarations.
        // Structs: `type X = {...}` → `export interface X {...}`
        // Enums/unions: `type X = "a" | "b"` → `export type X = "a" | "b"`
        macro_rules! collect_ts {
            ($($t:ty),+ $(,)?) => {
                $(
                    let decl = <$t>::decl(&cfg);
                    let is_struct_decl = decl.contains(" = {") && !decl.contains("} |");
                    let decl = if is_struct_decl {
                        // Struct → export interface
                        decl
                            .replacen("type ", "export interface ", 1)
                            .replace(" = {", " {")
                            .trim_end_matches(';')
                            .to_string()
                    } else {
                        // Enum/union → export type
                        decl
                            .replacen("type ", "export type ", 1)
                            .trim_end_matches(';')
                            .to_string()
                    };
                    parts.push(decl);
                    parts.push(String::new());
                )+
            };
        }

        collect_ts!(
            // Shared enums
            SortOrder,
            TimeRange,
            LinkType,
            // Auth
            AuthRegisterRequest,
            LoginRequest,
            AuthTokenResponse,
            RefreshRequest,
            LogoutRequest,
            ChangePasswordRequest,
            VerifyResponse,
            UserSettingsResponse,
            OkResponse,
            IssueApiKeyResponse,
            GitCredentialSummary,
            ListGitCredentialsResponse,
            CreateGitCredentialRequest,
            OAuthLinkResponse,
            // Sessions
            UploadResponse,
            SessionSummary,
            SessionListResponse,
            SessionListQuery,
            DesktopSessionListQuery,
            SessionRepoListResponse,
            DesktopHandoffBuildRequest,
            DesktopHandoffBuildResponse,
            DesktopQuickShareRequest,
            DesktopQuickShareResponse,
            DesktopContractVersionResponse,
            DesktopSummaryProviderId,
            DesktopSummaryProviderTransport,
            DesktopSummarySourceMode,
            DesktopSummaryResponseStyle,
            DesktopSummaryOutputShape,
            DesktopSummaryTriggerMode,
            DesktopSummaryStorageBackend,
            DesktopSummaryBatchExecutionMode,
            DesktopSummaryBatchScope,
            DesktopRuntimeSummaryProviderSettings,
            DesktopRuntimeSummaryPromptSettings,
            DesktopRuntimeSummaryResponseSettings,
            DesktopRuntimeSummaryStorageSettings,
            DesktopRuntimeSummaryBatchSettings,
            DesktopRuntimeSummarySettings,
            DesktopRuntimeSummaryProviderSettingsUpdate,
            DesktopRuntimeSummaryPromptSettingsUpdate,
            DesktopRuntimeSummaryResponseSettingsUpdate,
            DesktopRuntimeSummaryStorageSettingsUpdate,
            DesktopRuntimeSummaryBatchSettingsUpdate,
            DesktopRuntimeSummarySettingsUpdate,
            DesktopRuntimeSummaryUiConstraints,
            DesktopVectorSearchProvider,
            DesktopVectorSearchGranularity,
            DesktopVectorChunkingMode,
            DesktopVectorInstallState,
            DesktopVectorIndexState,
            DesktopRuntimeVectorSearchSettings,
            DesktopRuntimeVectorSearchSettingsUpdate,
            DesktopChangeReaderScope,
            DesktopChangeReaderVoiceProvider,
            DesktopRuntimeChangeReaderVoiceSettings,
            DesktopRuntimeChangeReaderVoiceSettingsUpdate,
            DesktopRuntimeChangeReaderSettings,
            DesktopRuntimeChangeReaderSettingsUpdate,
            DesktopRuntimeLifecycleSettings,
            DesktopRuntimeLifecycleSettingsUpdate,
            DesktopLifecycleCleanupState,
            DesktopLifecycleCleanupStatusResponse,
            DesktopVectorPreflightResponse,
            DesktopVectorInstallStatusResponse,
            DesktopVectorIndexStatusResponse,
            DesktopSummaryBatchState,
            DesktopSummaryBatchStatusResponse,
            DesktopVectorSessionMatch,
            DesktopVectorSearchResponse,
            DesktopRuntimeSettingsResponse,
            DesktopRuntimeSettingsUpdateRequest,
            DesktopSummaryProviderDetectResponse,
            DesktopSessionSummaryResponse,
            DesktopChangeReadRequest,
            DesktopChangeReadResponse,
            DesktopChangeQuestionRequest,
            DesktopChangeReaderTtsRequest,
            DesktopChangeReaderTtsResponse,
            DesktopChangeQuestionResponse,
            DesktopApiError,
            SessionDetail,
            SessionLink,
            ParseSource,
            ParseCandidate,
            ParsePreviewRequest,
            ParsePreviewResponse,
            ParsePreviewErrorResponse,
            LocalReviewBundle,
            LocalReviewPrMeta,
            LocalReviewReviewerQa,
            LocalReviewReviewerDigest,
            LocalReviewCommit,
            LocalReviewLayerFileChange,
            LocalReviewSemanticSummary,
            LocalReviewSession,
            // OAuth
            oauth::AuthProvidersResponse,
            oauth::OAuthProviderInfo,
            oauth::LinkedProvider,
            // Health
            HealthResponse,
            CapabilitiesResponse,
            ApiError,
        );

        let content = parts.join("\n");

        // Write to file
        if let Some(parent) = out_dir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut file = std::fs::File::create(&out_dir)
            .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_dir.display(), e));
        file.write_all(content.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", out_dir.display(), e));

        println!("Generated TypeScript types at: {}", out_dir.display());
    }
}
