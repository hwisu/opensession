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
pub mod oauth;
#[cfg(feature = "backend")]
pub mod service;

// Re-export core HAIL types for convenience
pub use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext, Stats,
};

// ─── Shared Enums ────────────────────────────────────────────────────────────

/// Role within a team.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum TeamRole {
    Admin,
    Member,
}

impl TeamRole {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }
}

impl std::fmt::Display for TeamRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Status of a team invitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
}

impl InvitationStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
        }
    }
}

impl std::fmt::Display for InvitationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

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

/// Legacy register (nickname-only). Kept for backward compatibility with CLI.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RegisterRequest {
    pub nickname: String,
}

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

/// Returned on successful legacy register (nickname-only, CLI-compatible).
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RegisterResponse {
    pub user_id: String,
    pub nickname: String,
    pub api_key: String,
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
    pub api_key: String,
    pub created_at: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    /// Linked OAuth providers (generic — replaces github_username)
    #[serde(default)]
    pub oauth_providers: Vec<oauth::LinkedProvider>,
    /// Legacy: GitHub username (populated from oauth_providers for backward compat)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_username: Option<String>,
}

/// Generic success response for operations that don't return data.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OkResponse {
    pub ok: bool,
}

/// Response for API key regeneration.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RegenerateKeyResponse {
    pub api_key: String,
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
    pub team_id: Option<String>,
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
}

/// Returned on successful session upload — contains the new session ID and URL.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UploadResponse {
    pub id: String,
    pub url: String,
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
    pub team_id: String,
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
    pub team_id: Option<String>,
    /// Sort order (default: recent)
    pub sort: Option<SortOrder>,
    /// Time range filter (default: all)
    pub time_range: Option<TimeRange>,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

/// Single session detail returned by `GET /api/sessions/:id`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionDetail {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub summary: SessionSummary,
    pub team_name: Option<String>,
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

// ─── Teams ──────────────────────────────────────────────────────────────────

/// Request body for `POST /api/teams` — create a new team.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateTeamRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_public: Option<bool>,
}

/// Single team record returned by list and detail endpoints.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub created_by: String,
    pub created_at: String,
}

/// Returned by `GET /api/teams` — teams the authenticated user belongs to.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListTeamsResponse {
    pub teams: Vec<TeamResponse>,
}

/// Returned by `GET /api/teams/:id` — team info with member count and recent sessions.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamDetailResponse {
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub team: TeamResponse,
    pub member_count: i64,
    pub sessions: Vec<SessionSummary>,
}

/// Request body for `PUT /api/teams/:id` — partial team update.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_public: Option<bool>,
}

/// Query parameters for `GET /api/teams/:id/stats`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamStatsQuery {
    /// Time range filter (default: all)
    pub time_range: Option<TimeRange>,
}

/// Returned by `GET /api/teams/:id/stats` — aggregated team statistics.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamStatsResponse {
    pub team_id: String,
    pub time_range: TimeRange,
    pub totals: TeamStatsTotals,
    pub by_user: Vec<UserStats>,
    pub by_tool: Vec<ToolStats>,
}

/// Aggregate totals across all sessions in a team.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamStatsTotals {
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub tool_call_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

/// Per-user aggregated statistics within a team.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UserStats {
    pub user_id: String,
    pub nickname: String,
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

/// Per-tool aggregated statistics within a team.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ToolStats {
    pub tool: String,
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

/// Request body for `POST /api/teams/:id/keys`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateTeamInviteKeyRequest {
    pub role: Option<TeamRole>,
    /// Defaults to 7 days. Clamped to [1, 30].
    pub expires_in_days: Option<u32>,
}

/// Create response for team invite key generation.
/// `invite_key` is only returned once.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateTeamInviteKeyResponse {
    pub key_id: String,
    pub invite_key: String,
    pub role: TeamRole,
    pub expires_at: String,
}

/// Team invite key metadata, safe to list repeatedly.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TeamInviteKeySummary {
    pub id: String,
    pub role: TeamRole,
    pub created_by_nickname: String,
    pub created_at: String,
    pub expires_at: String,
    pub used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// Returned by `GET /api/teams/:id/keys`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListTeamInviteKeysResponse {
    pub keys: Vec<TeamInviteKeySummary>,
}

/// Request body for `POST /api/teams/join-with-key`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JoinTeamWithKeyRequest {
    pub invite_key: String,
}

/// Returned after successful key redemption.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct JoinTeamWithKeyResponse {
    pub team_id: String,
    pub team_name: String,
    pub role: TeamRole,
}

// ─── From impls: core stats → API types ─────────────────────────────────────

impl From<opensession_core::stats::SessionAggregate> for TeamStatsTotals {
    fn from(a: opensession_core::stats::SessionAggregate) -> Self {
        Self {
            session_count: saturating_i64(a.session_count),
            message_count: saturating_i64(a.message_count),
            event_count: saturating_i64(a.event_count),
            tool_call_count: saturating_i64(a.tool_call_count),
            duration_seconds: saturating_i64(a.duration_seconds),
            total_input_tokens: saturating_i64(a.total_input_tokens),
            total_output_tokens: saturating_i64(a.total_output_tokens),
        }
    }
}

impl From<(String, opensession_core::stats::SessionAggregate)> for ToolStats {
    fn from((tool, a): (String, opensession_core::stats::SessionAggregate)) -> Self {
        Self {
            tool,
            session_count: saturating_i64(a.session_count),
            message_count: saturating_i64(a.message_count),
            event_count: saturating_i64(a.event_count),
            duration_seconds: saturating_i64(a.duration_seconds),
            total_input_tokens: saturating_i64(a.total_input_tokens),
            total_output_tokens: saturating_i64(a.total_output_tokens),
        }
    }
}

// ─── Invitations ─────────────────────────────────────────────────────────────

/// Request body for `POST /api/teams/:id/invite` — invite a user by email or OAuth identity.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct InviteRequest {
    pub email: Option<String>,
    /// OAuth provider name (e.g., "github", "gitlab").
    pub oauth_provider: Option<String>,
    /// Username on the OAuth provider (e.g., "octocat").
    pub oauth_provider_username: Option<String>,
    pub role: Option<TeamRole>,
}

/// Single invitation record returned by list and detail endpoints.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct InvitationResponse {
    pub id: String,
    pub team_id: String,
    pub team_name: String,
    pub email: Option<String>,
    pub oauth_provider: Option<String>,
    pub oauth_provider_username: Option<String>,
    pub invited_by_nickname: String,
    pub role: TeamRole,
    pub status: InvitationStatus,
    pub created_at: String,
}

/// Returned by `GET /api/invitations` — pending invitations for the current user.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListInvitationsResponse {
    pub invitations: Vec<InvitationResponse>,
}

/// Returned by `POST /api/invitations/:id/accept` — confirms team join.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AcceptInvitationResponse {
    pub team_id: String,
    pub role: TeamRole,
}

// ─── Members (admin-managed) ────────────────────────────────────────────────

/// Request body for `POST /api/teams/:id/members` — add a member by nickname.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AddMemberRequest {
    pub nickname: String,
}

/// Single team member record.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MemberResponse {
    pub user_id: String,
    pub nickname: String,
    pub role: TeamRole,
    pub joined_at: String,
}

/// Returned by `GET /api/teams/:id/members`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListMembersResponse {
    pub members: Vec<MemberResponse>,
}

// ─── Config Sync ─────────────────────────────────────────────────────────────

/// Team-level configuration synced to CLI clients.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ConfigSyncResponse {
    pub privacy: Option<SyncedPrivacyConfig>,
    pub watchers: Option<SyncedWatcherConfig>,
}

/// Privacy settings synced from the team — controls what data is recorded.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SyncedPrivacyConfig {
    pub exclude_patterns: Option<Vec<String>>,
    pub exclude_tools: Option<Vec<String>>,
}

/// Watcher toggle settings synced from the team — which tools to monitor.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SyncedWatcherConfig {
    pub claude_code: Option<bool>,
    pub opencode: Option<bool>,
    pub goose: Option<bool>,
    pub aider: Option<bool>,
    pub cursor: Option<bool>,
}

// ─── Sync ────────────────────────────────────────────────────────────────────

/// Query parameters for `GET /api/sync/pull` — cursor-based session sync.
#[derive(Debug, Deserialize)]
pub struct SyncPullQuery {
    pub team_id: String,
    /// Cursor: uploaded_at of the last received session
    pub since: Option<String>,
    /// Max sessions per page (default 100)
    pub limit: Option<u32>,
}

/// Returned by `GET /api/sync/pull` — paginated session data with cursor.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncPullResponse {
    pub sessions: Vec<SessionSummary>,
    /// Cursor for the next page (None = no more data)
    pub next_cursor: Option<String>,
    pub has_more: bool,
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

// ─── Error (legacy JSON shape) ──────────────────────────────────────────────

/// Legacy JSON error shape `{ "error": "..." }` returned by all error responses.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ApiError {
    pub error: String,
}

impl From<&ServiceError> for ApiError {
    fn from(e: &ServiceError) -> Self {
        Self {
            error: e.message().to_string(),
        }
    }
}

// ─── TypeScript generation ───────────────────────────────────────────────────

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
                    let decl = if decl.contains(" = {") {
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
            TeamRole,
            InvitationStatus,
            SortOrder,
            TimeRange,
            LinkType,
            // Auth
            RegisterRequest,
            AuthRegisterRequest,
            LoginRequest,
            AuthTokenResponse,
            RefreshRequest,
            LogoutRequest,
            ChangePasswordRequest,
            RegisterResponse,
            VerifyResponse,
            UserSettingsResponse,
            OkResponse,
            RegenerateKeyResponse,
            OAuthLinkResponse,
            // Sessions
            UploadResponse,
            SessionSummary,
            SessionListResponse,
            SessionListQuery,
            SessionDetail,
            SessionLink,
            // Teams
            CreateTeamRequest,
            TeamResponse,
            ListTeamsResponse,
            TeamDetailResponse,
            UpdateTeamRequest,
            TeamStatsQuery,
            TeamStatsResponse,
            TeamStatsTotals,
            UserStats,
            ToolStats,
            CreateTeamInviteKeyRequest,
            CreateTeamInviteKeyResponse,
            TeamInviteKeySummary,
            ListTeamInviteKeysResponse,
            JoinTeamWithKeyRequest,
            JoinTeamWithKeyResponse,
            // Members
            AddMemberRequest,
            MemberResponse,
            ListMembersResponse,
            // Invitations
            InviteRequest,
            InvitationResponse,
            ListInvitationsResponse,
            AcceptInvitationResponse,
            // OAuth
            oauth::AuthProvidersResponse,
            oauth::OAuthProviderInfo,
            oauth::LinkedProvider,
            // Health
            HealthResponse,
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
