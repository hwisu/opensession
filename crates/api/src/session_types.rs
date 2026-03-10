use crate::job_types::{JobContext, JobProtocol, JobReviewKind, JobStage, JobStatus};
use crate::shared_types::{LinkType, SortOrder, TimeRange};
use opensession_core::trace::{Agent, Event, Session, SessionContext};
use serde::{Deserialize, Serialize};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_context: Option<JobContext>,
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
    pub git_repo_name: Option<String>,
    pub protocol: Option<JobProtocol>,
    pub job_id: Option<String>,
    pub run_id: Option<String>,
    pub stage: Option<JobStage>,
    pub review_kind: Option<JobReviewKind>,
    pub status: Option<JobStatus>,
    pub sort: Option<SortOrder>,
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
    pub protocol: Option<String>,
    pub job_id: Option<String>,
    pub run_id: Option<String>,
    pub stage: Option<String>,
    pub review_kind: Option<String>,
    pub status: Option<String>,
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
            && self
                .job_id
                .as_deref()
                .is_none_or(|job_id| job_id.trim().is_empty())
            && self
                .run_id
                .as_deref()
                .is_none_or(|run_id| run_id.trim().is_empty())
            && self.protocol.is_none()
            && self.stage.is_none()
            && self.review_kind.is_none()
            && self.status.is_none()
            && self.page <= 10
            && self.per_page <= 50
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_query() -> SessionListQuery {
        SessionListQuery {
            page: 1,
            per_page: 20,
            search: None,
            tool: None,
            git_repo_name: None,
            protocol: None,
            job_id: None,
            run_id: None,
            stage: None,
            review_kind: None,
            status: None,
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
