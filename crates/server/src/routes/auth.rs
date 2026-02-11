use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{request::Parts, StatusCode},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    crypto, db as dbq, oauth, service, AuthRegisterRequest, AuthTokenResponse,
    ChangePasswordRequest, LoginRequest, LogoutRequest, OkResponse, RefreshRequest,
    RegenerateKeyResponse, RegisterRequest, RegisterResponse, UserSettingsResponse, VerifyResponse,
};

use crate::error::ApiErr;
use crate::storage::Db;
use crate::AppConfig;

// ---------------------------------------------------------------------------
// Auth extractor — JWT + API key dual auth
// ---------------------------------------------------------------------------

/// Authenticated user extracted from `Authorization: Bearer <token>`.
///
/// Priority:
/// 1. `osk_` prefix → API key DB lookup (legacy)
/// 2. Otherwise → JWT verify → user_id DB lookup
pub struct AuthUser {
    pub user_id: String,
    pub nickname: String,
    pub is_admin: bool,
    pub email: Option<String>,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Db: FromRef<S>,
    AppConfig: FromRef<S>,
{
    type Rejection = ApiErr;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let db = Db::from_ref(state);
        let config = AppConfig::from_ref(state);

        let token = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ApiErr::unauthorized(
                "missing or invalid Authorization header",
            ))?
            .to_string();

        // API key path
        if token.starts_with("osk_") {
            let conn = db.conn();
            return conn
                .query_row(dbq::USER_BY_API_KEY, [&token], |row| {
                    Ok(AuthUser {
                        user_id: row.get(0)?,
                        nickname: row.get(1)?,
                        is_admin: row.get(2)?,
                        email: row.get(3)?,
                    })
                })
                .map_err(|_| ApiErr::unauthorized("invalid API key"));
        }

        // JWT path
        if config.jwt_secret.is_empty() {
            return Err(ApiErr::unauthorized("JWT authentication not configured"));
        }

        let now = chrono::Utc::now().timestamp() as u64;
        let user_id = crypto::verify_jwt(&token, &config.jwt_secret, now)
            .map_err(|e| ApiErr::unauthorized(e.message()))?;

        let conn = db.conn();
        conn.query_row(dbq::USER_BY_ID, [&user_id], |row| {
            Ok(AuthUser {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                is_admin: row.get(2)?,
                email: row.get(3)?,
            })
        })
        .map_err(|_| ApiErr::unauthorized("user not found"))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Public wrapper for oauth module to issue tokens.
pub fn issue_tokens_pub(
    db: &Db,
    jwt_secret: &str,
    user_id: &str,
    nickname: &str,
) -> Result<AuthTokenResponse, ApiErr> {
    issue_tokens(db, jwt_secret, user_id, nickname)
}

fn issue_tokens(
    db: &Db,
    jwt_secret: &str,
    user_id: &str,
    nickname: &str,
) -> Result<AuthTokenResponse, ApiErr> {
    let now = chrono::Utc::now().timestamp() as u64;
    let bundle = service::prepare_token_bundle(jwt_secret, user_id, nickname, now);

    let conn = db.conn();
    conn.execute(
        dbq::REFRESH_TOKEN_INSERT,
        rusqlite::params![
            bundle.token_id,
            user_id,
            bundle.token_hash,
            bundle.expires_at
        ],
    )
    .map_err(ApiErr::from_db("issue_tokens"))?;

    Ok(bundle.response)
}

fn is_first_user(db: &Db) -> bool {
    let conn = db.conn();
    let count: i64 = conn
        .query_row(dbq::USER_COUNT, [], |row| row.get(0))
        .unwrap_or(0);
    count == 0
}

// ---------------------------------------------------------------------------
// Register — first user becomes admin (legacy, CLI-compatible)
// ---------------------------------------------------------------------------

pub async fn register(
    State(db): State<Db>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiErr> {
    let nickname = service::validate_nickname(&req.nickname).map_err(ApiErr::from)?;

    let user_id = Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();

    let conn = db.conn();

    let user_count: i64 = conn
        .query_row(dbq::USER_COUNT, [], |row| row.get(0))
        .unwrap_or(0);
    let is_admin = user_count == 0;

    let result = conn.execute(
        dbq::USER_INSERT,
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
// Email/password auth
// ---------------------------------------------------------------------------

/// POST /api/auth/register — email + password registration
pub async fn auth_register(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    Json(req): Json<AuthRegisterRequest>,
) -> Result<(StatusCode, Json<AuthTokenResponse>), ApiErr> {
    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    let email = service::validate_email(&req.email).map_err(ApiErr::from)?;
    service::validate_password(&req.password).map_err(ApiErr::from)?;
    let nickname = service::validate_nickname(&req.nickname).map_err(ApiErr::from)?;

    // Check email uniqueness
    {
        let conn = db.conn();
        let exists: bool = conn
            .query_row(dbq::USER_EMAIL_EXISTS, [&email], |row| row.get(0))
            .unwrap_or(false);
        if exists {
            return Err(ApiErr::conflict("email already registered"));
        }
    }

    let user_id = Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();
    let (password_hash, password_salt) = crypto::hash_password(&req.password);
    let admin = is_first_user(&db);

    {
        let conn = db.conn();
        let result = conn.execute(
            dbq::USER_INSERT_WITH_EMAIL,
            rusqlite::params![
                user_id,
                nickname,
                api_key,
                admin,
                email,
                password_hash,
                password_salt
            ],
        );
        match result {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(ApiErr::conflict("nickname already taken"));
            }
            Err(e) => {
                tracing::error!("auth_register error: {e}");
                return Err(ApiErr::internal("internal server error"));
            }
        }
    }

    let tokens = issue_tokens(&db, &config.jwt_secret, &user_id, &nickname)?;
    Ok((StatusCode::CREATED, Json(tokens)))
}

/// POST /api/auth/login — email + password login
pub async fn login(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthTokenResponse>, ApiErr> {
    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    let email = service::validate_email(&req.email).map_err(ApiErr::from)?;

    let conn = db.conn();
    let user = conn
        .query_row(dbq::USER_BY_EMAIL_FOR_LOGIN, [&email], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|_| ApiErr::unauthorized("invalid email or password"))?;

    let (user_id, nickname, hash, salt) = user;
    let (hash, salt) = match (hash, salt) {
        (Some(h), Some(s)) => (h, s),
        _ => {
            return Err(ApiErr::unauthorized(
                "this account uses OAuth login, not email/password",
            ))
        }
    };

    if !crypto::verify_password(&req.password, &hash, &salt) {
        return Err(ApiErr::unauthorized("invalid email or password"));
    }
    drop(conn);

    let tokens = issue_tokens(&db, &config.jwt_secret, &user_id, &nickname)?;
    Ok(Json(tokens))
}

/// POST /api/auth/refresh — exchange refresh token for new JWT
pub async fn refresh(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<AuthTokenResponse>, ApiErr> {
    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    let token_hash = crypto::hash_token(&req.refresh_token);

    let conn = db.conn();
    let row = conn
        .query_row(dbq::REFRESH_TOKEN_LOOKUP, [&token_hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| ApiErr::unauthorized("invalid refresh token"))?;

    let (rt_id, user_id, expires_at, nickname) = row;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if expires_at < now {
        conn.execute("DELETE FROM refresh_tokens WHERE id = ?1", [&rt_id])
            .ok();
        return Err(ApiErr::unauthorized("refresh token expired"));
    }

    // Rotate: delete old, issue new
    conn.execute(dbq::REFRESH_TOKEN_DELETE, [&token_hash]).ok();
    drop(conn);

    let tokens = issue_tokens(&db, &config.jwt_secret, &user_id, &nickname)?;
    Ok(Json(tokens))
}

/// POST /api/auth/logout — invalidate refresh token
pub async fn logout(
    State(db): State<Db>,
    Json(req): Json<LogoutRequest>,
) -> Result<Json<OkResponse>, ApiErr> {
    let token_hash = crypto::hash_token(&req.refresh_token);
    let conn = db.conn();
    conn.execute(dbq::REFRESH_TOKEN_DELETE, [&token_hash]).ok();
    Ok(Json(OkResponse { ok: true }))
}

/// PUT /api/auth/password — change password (authenticated)
pub async fn change_password(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();
    let (hash, salt): (Option<String>, Option<String>) = conn
        .query_row(dbq::USER_PASSWORD_FIELDS, [&user.user_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(ApiErr::from_db("change_password lookup"))?;

    let (hash, salt) = match (hash, salt) {
        (Some(h), Some(s)) => (h, s),
        _ => {
            return Err(ApiErr::bad_request(
                "cannot change password for OAuth-only account",
            ))
        }
    };

    if !crypto::verify_password(&req.current_password, &hash, &salt) {
        return Err(ApiErr::unauthorized("current password is incorrect"));
    }

    service::validate_password(&req.new_password).map_err(ApiErr::from)?;
    let (new_hash, new_salt) = crypto::hash_password(&req.new_password);

    conn.execute(
        dbq::USER_UPDATE_PASSWORD,
        rusqlite::params![new_hash, new_salt, user.user_id],
    )
    .map_err(ApiErr::from_db("change_password update"))?;

    Ok(Json(OkResponse { ok: true }))
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

/// POST /api/auth/verify — confirm token validity, return user info.
pub async fn verify(user: AuthUser) -> Json<VerifyResponse> {
    Json(VerifyResponse {
        user_id: user.user_id,
        nickname: user.nickname,
    })
}

// ---------------------------------------------------------------------------
// Get current user settings
// ---------------------------------------------------------------------------

/// GET /api/auth/me — return full profile for the authenticated user.
pub async fn me(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<UserSettingsResponse>, ApiErr> {
    let conn = db.conn();
    let (email, avatar_url): (Option<String>, Option<String>) = conn
        .query_row(dbq::USER_EMAIL_AVATAR, [&user.user_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(ApiErr::from_db("me error"))?;

    // Load linked OAuth providers
    let mut stmt = conn
        .prepare(dbq::OAUTH_IDENTITY_FIND_BY_USER)
        .map_err(ApiErr::from_db("me prepare"))?;
    let providers: Vec<oauth::LinkedProvider> = stmt
        .query_map([&user.user_id], |row| {
            Ok(oauth::LinkedProvider {
                provider: row.get(1)?,
                provider_username: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                display_name: match row.get::<_, String>(1)?.as_str() {
                    "github" => "GitHub".to_string(),
                    "gitlab" => "GitLab".to_string(),
                    other => other.to_string(),
                },
            })
        })
        .map_err(ApiErr::from_db("me query oauth"))?
        .filter_map(|r| r.ok())
        .collect();

    // Legacy: first GitHub provider's username
    let github_username = providers
        .iter()
        .find(|p| p.provider == "github")
        .map(|p| p.provider_username.clone());

    let (api_key, created_at): (String, String) = conn
        .query_row(dbq::USER_SETTINGS_FIELDS, [&user.user_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap_or_default();

    Ok(Json(UserSettingsResponse {
        user_id: user.user_id,
        nickname: user.nickname,
        api_key,
        is_admin: user.is_admin,
        created_at,
        email,
        avatar_url,
        oauth_providers: providers,
        github_username,
    }))
}

// ---------------------------------------------------------------------------
// Regenerate API key
// ---------------------------------------------------------------------------

/// POST /api/auth/regenerate-key — generate a new API key (invalidates the old one).
pub async fn regenerate_key(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<RegenerateKeyResponse>, ApiErr> {
    let new_key = service::generate_api_key();
    let conn = db.conn();
    conn.execute(
        dbq::USER_UPDATE_API_KEY,
        rusqlite::params![&new_key, &user.user_id],
    )
    .map_err(ApiErr::from_db("regenerate key error"))?;

    Ok(Json(RegenerateKeyResponse { api_key: new_key }))
}
