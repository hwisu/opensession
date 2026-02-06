use serde::{Deserialize, Serialize};
use worker::*;

/// D1 + R2 storage layer for the Cloudflare Worker.

// ── D1 helpers ──────────────────────────────────────────────────────────

/// Row returned when listing sessions.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub user_id: String,
    pub title: Option<String>,
    pub agent_provider: String,
    pub agent_model: String,
    pub agent_tool: String,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Row returned when listing groups.
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub created_at: String,
}

/// Row for group membership.
#[derive(Debug, Serialize, Deserialize)]
pub struct MemberRow {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub joined_at: String,
}

/// Row for a user record.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub api_key_hash: String,
    pub created_at: String,
}

/// Row for an invite.
#[derive(Debug, Serialize, Deserialize)]
pub struct InviteRow {
    pub id: String,
    pub group_id: String,
    pub code: String,
    pub created_by: String,
    pub expires_at: Option<String>,
}

// ── D1 accessor ─────────────────────────────────────────────────────────

pub fn get_d1(env: &Env) -> Result<D1Database> {
    env.d1("DB")
}

// ── R2 accessor ─────────────────────────────────────────────────────────

pub fn get_r2(env: &Env) -> Result<Bucket> {
    env.bucket("SESSIONS")
}

// ── R2 convenience functions ────────────────────────────────────────────

/// Store a raw session body (JSON bytes) in R2.
pub async fn put_session_body(env: &Env, session_id: &str, body: &[u8]) -> Result<()> {
    let bucket = get_r2(env)?;
    bucket.put(session_id, body.to_vec()).execute().await?;
    Ok(())
}

/// Retrieve a raw session body from R2.
pub async fn get_session_body(env: &Env, session_id: &str) -> Result<Option<Vec<u8>>> {
    let bucket = get_r2(env)?;
    match bucket.get(session_id).execute().await? {
        Some(object) => {
            let bytes = object.body().unwrap().bytes().await?;
            Ok(Some(bytes))
        }
        None => Ok(None),
    }
}

// ── D1 convenience functions ────────────────────────────────────────────

/// Authenticate a request by API key. Returns the user row if valid.
pub async fn authenticate(env: &Env, api_key: &str) -> Result<Option<UserRow>> {
    let db = get_d1(env)?;
    // We store a hash of the API key; for simplicity in the worker we compare directly.
    // In production you would hash the incoming key and compare hashes.
    let stmt = db.prepare("SELECT id, username, api_key_hash, created_at FROM users WHERE api_key_hash = ?1");
    let query = stmt.bind(&[api_key.into()])?;
    query.first::<UserRow>(None).await
}

/// Extract the bearer token from an Authorization header value.
pub fn extract_bearer(auth_header: &str) -> Option<&str> {
    let trimmed = auth_header.trim();
    if trimmed.starts_with("Bearer ") {
        Some(trimmed[7..].trim())
    } else {
        None
    }
}
