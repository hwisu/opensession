use serde::{Deserialize, Serialize};
use worker::*;

use opensession_api::service::AuthToken;

// D1 + R2 storage layer for the Cloudflare Worker.

// ── D1 bool helper ─────────────────────────────────────────────────────────

/// D1 returns booleans as floats (0.0 / 1.0). This deserializer handles both.
pub fn bool_from_d1<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<bool, D::Error> {
    use serde::de;
    struct BoolVisitor;
    impl<'de> de::Visitor<'de> for BoolVisitor {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or number")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<bool, E> {
            Ok(v)
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<bool, E> {
            Ok(v != 0.0)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<bool, E> {
            Ok(v != 0)
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<bool, E> {
            Ok(v != 0)
        }
    }
    d.deserialize_any(BoolVisitor)
}

// ── Row types ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct UserRow {
    pub id: String,
    pub nickname: String,
    pub api_key: String,
    pub created_at: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub password_salt: Option<String>,
    pub github_id: Option<String>,
    pub github_username: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub team_id: String,
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
    #[serde(default)]
    pub git_remote: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default)]
    pub git_repo_name: Option<String>,
    #[serde(default)]
    pub pr_number: Option<i64>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub files_modified: Option<String>,
    #[serde(default)]
    pub files_read: Option<String>,
    #[serde(default, deserialize_with = "bool_from_d1")]
    pub has_errors: bool,
    #[serde(default = "default_max_active_agents")]
    pub max_active_agents: i64,
}

fn default_max_active_agents() -> i64 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TeamRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(deserialize_with = "bool_from_d1")]
    pub is_public: bool,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemberRow {
    pub user_id: String,
    pub nickname: String,
    pub role: String,
    pub joined_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CountRow {
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvitationRow {
    pub id: String,
    pub team_id: String,
    pub team_name: String,
    pub email: Option<String>,
    pub oauth_provider: Option<String>,
    pub oauth_provider_username: Option<String>,
    pub invited_by_nickname: String,
    pub role: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
}

// ── Helper row types (D1 deserialization) ───────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct RefreshTokenRow {
    pub id: String,
    pub user_id: String,
    pub expires_at: String,
    pub nickname: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthStateRow {
    #[allow(dead_code)]
    pub state: String,
    pub provider: String,
    pub expires_at: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OAuthIdentityRow {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NicknameRow {
    pub nickname: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderRow {
    pub provider: String,
    pub provider_username: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RoleRow {
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreatedAtRow {
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserIdRow {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JoinedAtRow {
    pub joined_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TeamNameRow {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InvitationLookupRow {
    #[allow(dead_code)]
    pub id: String,
    pub team_id: String,
    pub email: Option<String>,
    pub oauth_provider: Option<String>,
    pub oauth_provider_username: Option<String>,
    pub role: String,
    pub status: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatsTotalsRow {
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserStatsRow {
    pub user_id: String,
    pub nickname: String,
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolStatsRow {
    pub tool: String,
    pub session_count: i64,
    pub message_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StorageInfoRow {
    pub body_storage_key: String,
    pub body_url: Option<String>,
}

// ── D1 accessor ─────────────────────────────────────────────────────────────

pub fn get_d1(env: &Env) -> Result<D1Database> {
    env.d1("DB")
}

// ── R2 accessor ─────────────────────────────────────────────────────────────

pub fn get_r2(env: &Env) -> Result<Bucket> {
    env.bucket("SESSIONS")
}

// ── R2 convenience functions ────────────────────────────────────────────────

/// Store a raw session body (HAIL JSONL bytes) in R2.
pub async fn put_session_body(env: &Env, key: &str, body: &[u8]) -> Result<()> {
    let bucket = get_r2(env)?;
    bucket.put(key, body.to_vec()).execute().await?;
    Ok(())
}

/// Retrieve a raw session body from R2.
pub async fn get_session_body(env: &Env, key: &str) -> Result<Option<Vec<u8>>> {
    let bucket = get_r2(env)?;
    match bucket.get(key).execute().await? {
        Some(object) => {
            let bytes = object.body().unwrap().bytes().await?;
            Ok(Some(bytes))
        }
        None => Ok(None),
    }
}

// ── Auth helpers ────────────────────────────────────────────────────────────
//
// Worker-specific exception: these use inline `SELECT * FROM users` queries
// instead of `api::db::users` builders because:
// 1. D1 execution model (prepare/bind/first) differs from rusqlite
// 2. UserRow requires all columns (`SELECT *`), while api builders select subsets
// 3. D1 deserializes directly into typed structs via serde, not row callbacks

/// Look up a user by API key.
pub async fn authenticate(env: &Env, api_key: &str) -> Result<Option<UserRow>> {
    let db = get_d1(env)?;
    db.prepare("SELECT * FROM users WHERE api_key = ?1")
        .bind(&[api_key.into()])?
        .first::<UserRow>(None)
        .await
}

/// Look up a user by id (for JWT-based auth).
pub async fn get_user_by_id(env: &Env, user_id: &str) -> Result<Option<UserRow>> {
    let db = get_d1(env)?;
    db.prepare("SELECT * FROM users WHERE id = ?1")
        .bind(&[user_id.into()])?
        .first::<UserRow>(None)
        .await
}

/// Look up a user by email.
pub async fn get_user_by_email(env: &Env, email: &str) -> Result<Option<UserRow>> {
    let db = get_d1(env)?;
    db.prepare("SELECT * FROM users WHERE email = ?1")
        .bind(&[email.into()])?
        .first::<UserRow>(None)
        .await
}

/// Extract the bearer token from an Authorization header value.
pub fn extract_bearer(auth_header: &str) -> Option<&str> {
    auth_header.trim().strip_prefix("Bearer ").map(|s| s.trim())
}

/// Extract JWT from cookie header. Looks for `session=<token>`.
fn extract_jwt_from_cookie(cookie_header: &str) -> Option<&str> {
    cookie_header.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("session=")
    })
}

/// Get current unix timestamp via `Date.now()`.
pub fn now_unix() -> u64 {
    (chrono::Utc::now().timestamp()) as u64
}

/// Dual-auth middleware: JWT (cookie or header) → API key → 401.
///
/// Priority:
/// 1. `Cookie: session=<jwt>` or `Authorization: Bearer <jwt>` (non-osk_ prefix)
/// 2. `Authorization: Bearer osk_xxx` → API key DB lookup
/// 3. Both missing → 401
pub async fn auth_from_req(req: &Request, env: &Env) -> Result<UserRow> {
    let headers = req.headers();

    // Extract token from cookie or Authorization header
    let token = if let Ok(Some(cookie)) = headers.get("Cookie") {
        extract_jwt_from_cookie(&cookie).map(|s| s.to_string())
    } else {
        None
    }
    .or_else(|| {
        headers
            .get("Authorization")
            .ok()
            .flatten()
            .and_then(|auth| extract_bearer(&auth).map(|s| s.to_string()))
    })
    .ok_or_else(|| Error::from("Unauthorized"))?;

    let secret = env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .unwrap_or_default();

    let resolved = opensession_api::service::resolve_auth_token(&token, &secret, now_unix())
        .map_err(|e| Error::from(e.message().to_string()))?;

    match resolved {
        AuthToken::ApiKey(key) => authenticate(env, &key)
            .await?
            .ok_or_else(|| Error::from("Unauthorized")),
        AuthToken::Jwt(user_id) => get_user_by_id(env, &user_id)
            .await?
            .ok_or_else(|| Error::from("User not found")),
    }
}

/// Wrapper that returns a proper 401 response on auth failure.
pub async fn require_auth(req: &Request, env: &Env) -> std::result::Result<UserRow, Response> {
    auth_from_req(req, env).await.map_err(|e| {
        let msg = e.to_string();
        Response::error(&msg, 401).unwrap_or_else(|_| Response::error("Unauthorized", 401).unwrap())
    })
}
