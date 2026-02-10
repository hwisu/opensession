use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{request::Parts, StatusCode},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    service, RegisterRequest, RegisterResponse, UserSettingsResponse, VerifyResponse,
};

use crate::error::ApiErr;
use crate::storage::Db;

// ---------------------------------------------------------------------------
// Auth extractor
// ---------------------------------------------------------------------------

/// Authenticated user extracted from the `Authorization: Bearer <api_key>` header.
pub struct AuthUser {
    pub user_id: String,
    pub nickname: String,
    pub is_admin: bool,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Db: axum::extract::FromRef<S>,
{
    type Rejection = ApiErr;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let db = Db::from_ref(state);

        let api_key = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ApiErr::unauthorized(
                "missing or invalid Authorization header",
            ))?
            .to_string();

        let conn = db.conn();
        conn.query_row(
            "SELECT id, nickname, is_admin FROM users WHERE api_key = ?1",
            [&api_key],
            |row| {
                Ok(AuthUser {
                    user_id: row.get(0)?,
                    nickname: row.get(1)?,
                    is_admin: row.get(2)?,
                })
            },
        )
        .map_err(|_| ApiErr::unauthorized("invalid API key"))
    }
}

// ---------------------------------------------------------------------------
// Register — first user becomes admin
// ---------------------------------------------------------------------------

pub async fn register(
    State(db): State<Db>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiErr> {
    let nickname = service::validate_nickname(&req.nickname).map_err(ApiErr::from)?;

    let user_id = Uuid::new_v4().to_string();
    let api_key = format!("osk_{}", Uuid::new_v4().simple());

    let conn = db.conn();

    // First user becomes admin
    let user_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .unwrap_or(0);
    let is_admin = user_count == 0;

    let result = conn.execute(
        "INSERT INTO users (id, nickname, api_key, is_admin) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&user_id, &nickname, &api_key, is_admin],
    );

    match result {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(RegisterResponse {
                user_id,
                nickname,
                api_key,
                is_admin,
            }),
        )),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(ApiErr::conflict("nickname already taken"))
        }
        Err(e) => {
            tracing::error!("register error: {e}");
            Err(ApiErr::internal("internal server error"))
        }
    }
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

pub async fn verify(user: AuthUser) -> Json<VerifyResponse> {
    Json(VerifyResponse {
        user_id: user.user_id,
        nickname: user.nickname,
    })
}

// ---------------------------------------------------------------------------
// Get current user settings
// ---------------------------------------------------------------------------

pub async fn me(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<UserSettingsResponse>, ApiErr> {
    let conn = db.conn();
    conn.query_row(
        "SELECT id, nickname, api_key, is_admin, created_at FROM users WHERE id = ?1",
        [&user.user_id],
        |row| {
            Ok(UserSettingsResponse {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                api_key: row.get(2)?,
                is_admin: row.get(3)?,
                created_at: row.get(4)?,
            })
        },
    )
    .map(Json)
    .map_err(ApiErr::from_db("me error"))
}

// ---------------------------------------------------------------------------
// Regenerate API key
// ---------------------------------------------------------------------------

pub async fn regenerate_key(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, ApiErr> {
    let new_key = format!("osk_{}", Uuid::new_v4().simple());
    let conn = db.conn();
    conn.execute(
        "UPDATE users SET api_key = ?1 WHERE id = ?2",
        rusqlite::params![&new_key, &user.user_id],
    )
    .map_err(ApiErr::from_db("regenerate key error"))?;

    Ok(Json(serde_json::json!({ "api_key": new_key })))
}
