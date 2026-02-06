//! Shared API types for opensession.io
//!
//! This crate is the **single source of truth** for all API request/response types.
//! The server (Axum) and worker (Cloudflare) import these types directly.
//! TypeScript types are auto-generated via `ts-rs` and consumed by the frontend.
//!
//! To regenerate TypeScript types:
//!   cargo test -p opensession-api-types -- export_typescript --nocapture

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// Re-export core HAIL types for convenience
pub use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext, Stats,
};

// ─── Auth ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RegisterRequest {
    pub nickname: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RegisterResponse {
    pub user_id: String,
    pub nickname: String,
    pub api_key: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct VerifyResponse {
    pub user_id: String,
    pub nickname: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct UserSettingsResponse {
    pub user_id: String,
    pub nickname: String,
    pub api_key: String,
    pub github_login: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: String,
}

// ─── Sessions ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct UploadRequest {
    #[ts(type = "any")]
    pub session: serde_json::Value, // Full HAIL Session JSON
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub group_ids: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct UploadResponse {
    pub id: String,
    pub url: String,
}

/// Flat session summary returned by list/detail endpoints.
/// This is NOT the full HAIL Session — it's a DB-derived summary.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
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
    pub visibility: String,
    pub created_at: String,
    pub uploaded_at: String,
    pub message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SessionListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    pub search: Option<String>,
    pub tool: Option<String>,
    pub group_id: Option<String>,
    /// Sort order: "recent" (default), "popular" (message_count desc), "longest"
    pub sort: Option<String>,
    /// Time range filter: "24h", "7d", "30d", "all" (default)
    pub time_range: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SessionDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub summary: SessionSummary,
    pub groups: Vec<GroupRef>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct GroupRef {
    pub id: String,
    pub name: String,
}

// ─── Groups ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct GroupResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    pub owner_id: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ListGroupsResponse {
    pub groups: Vec<GroupResponse>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct GroupDetailResponse {
    #[serde(flatten)]
    #[ts(flatten)]
    pub group: GroupResponse,
    pub member_count: i64,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_public: Option<bool>,
}

// ─── Invites ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CreateInviteRequest {
    pub max_uses: Option<i64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct InviteResponse {
    pub id: String,
    pub group_id: String,
    pub code: String,
    pub max_uses: Option<i64>,
    pub used_count: i64,
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct JoinResponse {
    pub group_id: String,
    pub group_name: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MemberResponse {
    pub user_id: String,
    pub nickname: String,
    pub role: String,
    pub joined_at: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ListMembersResponse {
    pub members: Vec<MemberResponse>,
}

// ─── Health ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ApiError {
    pub error: String,
}

// ─── TypeScript generation ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// Run with: cargo test -p opensession-api-types -- export_typescript --nocapture
    #[test]
    fn export_typescript() {
        let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/lib/api-types.generated.ts");

        let cfg = ts_rs::Config::new().with_large_int("number");
        let mut parts: Vec<String> = Vec::new();
        parts.push("// AUTO-GENERATED by opensession-api-types — DO NOT EDIT".to_string());
        parts.push("// Regenerate with: cargo test -p opensession-api-types -- export_typescript".to_string());
        parts.push(String::new());

        // Collect all type declarations, converting `type X = {...}` to `export interface X {...}`
        macro_rules! collect_ts {
            ($($t:ty),+ $(,)?) => {
                $(
                    let decl = <$t>::decl(&cfg);
                    // Convert `type Foo = { ... };` to `export interface Foo { ... }`
                    let decl = decl
                        .replacen("type ", "export interface ", 1)
                        .replace(" = {", " {")
                        .trim_end_matches(';')
                        .to_string();
                    parts.push(decl);
                    parts.push(String::new());
                )+
            };
        }

        collect_ts!(
            // Auth
            RegisterRequest,
            RegisterResponse,
            VerifyResponse,
            UserSettingsResponse,
            // Sessions
            UploadResponse,
            SessionSummary,
            SessionListResponse,
            SessionListQuery,
            SessionDetail,
            GroupRef,
            // Groups
            CreateGroupRequest,
            GroupResponse,
            ListGroupsResponse,
            GroupDetailResponse,
            UpdateGroupRequest,
            // Invites
            CreateInviteRequest,
            InviteResponse,
            JoinResponse,
            MemberResponse,
            ListMembersResponse,
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
