use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{RegisterRequest, RegisterResponse, UserSettingsResponse, VerifyResponse};

use crate::storage::Db;

// ---------------------------------------------------------------------------
// Auth extractor
// ---------------------------------------------------------------------------

/// Authenticated user extracted from the `Authorization: Bearer <api_key>` header.
pub struct AuthUser {
    pub user_id: String,
    pub nickname: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Db: axum::extract::FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let db = Db::from_ref(state);

        let api_key = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "missing or invalid Authorization header"})),
                )
                    .into_response()
            })?
            .to_string();

        let conn = db.conn();
        let result = conn.query_row(
            "SELECT id, nickname FROM users WHERE api_key = ?1",
            [&api_key],
            |row| {
                Ok(AuthUser {
                    user_id: row.get(0)?,
                    nickname: row.get(1)?,
                })
            },
        );

        match result {
            Ok(user) => Ok(user),
            Err(_) => Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid API key"})),
            )
                .into_response()),
        }
    }
}

// ---------------------------------------------------------------------------
// Register
// ---------------------------------------------------------------------------

pub async fn register(
    State(db): State<Db>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), Response> {
    let nickname = req.nickname.trim().to_string();
    if nickname.is_empty() || nickname.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "nickname must be 1-64 characters"})),
        )
            .into_response());
    }

    // Check registration is enabled
    let registration = std::env::var("OPENSESSION_REGISTRATION").unwrap_or_default();
    if registration == "closed" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "registration is currently closed"})),
        )
            .into_response());
    }

    let user_id = Uuid::new_v4().to_string();
    let api_key = format!("osk_{}", Uuid::new_v4().simple());

    let conn = db.conn();
    let result = conn.execute(
        "INSERT INTO users (id, nickname, api_key) VALUES (?1, ?2, ?3)",
        rusqlite::params![&user_id, &nickname, &api_key],
    );

    match result {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(RegisterResponse {
                user_id,
                nickname,
                api_key,
            }),
        )),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "nickname already taken"})),
            )
                .into_response())
        }
        Err(e) => {
            tracing::error!("register error: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal server error"})),
            )
                .into_response())
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
) -> Result<Json<UserSettingsResponse>, Response> {
    let conn = db.conn();
    conn.query_row(
        "SELECT id, nickname, api_key, github_login, avatar_url, created_at FROM users WHERE id = ?1",
        [&user.user_id],
        |row| {
            Ok(UserSettingsResponse {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                api_key: row.get(2)?,
                github_login: row.get(3)?,
                avatar_url: row.get(4)?,
                created_at: row.get(5)?,
            })
        },
    )
    .map(Json)
    .map_err(|e| {
        tracing::error!("me error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal server error"})),
        )
            .into_response()
    })
}

// ---------------------------------------------------------------------------
// Regenerate API key
// ---------------------------------------------------------------------------

pub async fn regenerate_key(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<serde_json::Value>, Response> {
    let new_key = format!("osk_{}", Uuid::new_v4().simple());
    let conn = db.conn();
    conn.execute(
        "UPDATE users SET api_key = ?1 WHERE id = ?2",
        rusqlite::params![&new_key, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("regenerate key error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal server error"})),
        )
            .into_response()
    })?;

    Ok(Json(serde_json::json!({ "api_key": new_key })))
}
