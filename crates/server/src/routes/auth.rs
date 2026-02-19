use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{request::Parts, StatusCode},
    Json,
};
use uuid::Uuid;

use opensession_api::{
    crypto, db as dbq, oauth, service, service::AuthToken, AuthRegisterRequest, AuthTokenResponse,
    ChangePasswordRequest, LoginRequest, LogoutRequest, OkResponse, RefreshRequest,
    RegenerateKeyResponse, RegisterRequest, RegisterResponse, UserSettingsResponse, VerifyResponse,
};

use crate::error::ApiErr;
use crate::storage::{sq_execute, sq_query_map, sq_query_row, Db};
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
    #[allow(dead_code)]
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
            ))?;

        let now = chrono::Utc::now().timestamp() as u64;
        let resolved = service::resolve_auth_token(token, &config.jwt_secret, now)
            .map_err(|e| ApiErr::unauthorized(e.message()))?;

        let conn = db.conn();
        match resolved {
            AuthToken::ApiKey(key) => {
                sq_query_row(&conn, dbq::users::get_by_api_key(&key), |row| {
                    Ok(AuthUser {
                        user_id: row.get(0)?,
                        nickname: row.get(1)?,
                        email: row.get(2)?,
                    })
                })
                .map_err(|_| ApiErr::unauthorized("invalid API key"))
            }
            AuthToken::Jwt(user_id) => {
                sq_query_row(&conn, dbq::users::get_by_id(&user_id), |row| {
                    Ok(AuthUser {
                        user_id: row.get(0)?,
                        nickname: row.get(1)?,
                        email: row.get(2)?,
                    })
                })
                .map_err(|_| ApiErr::unauthorized("user not found"))
            }
        }
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
    let bundle =
        service::prepare_token_bundle(jwt_secret, user_id, nickname, now).map_err(ApiErr::from)?;

    let conn = db.conn();
    sq_execute(
        &conn,
        dbq::users::insert_refresh_token(
            &bundle.token_id,
            user_id,
            &bundle.token_hash,
            &bundle.expires_at,
        ),
    )
    .map_err(ApiErr::from_db("issue_tokens"))?;

    Ok(bundle.response)
}

// ---------------------------------------------------------------------------
// Register (legacy, CLI-compatible)
// ---------------------------------------------------------------------------

pub async fn register(
    State(db): State<Db>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiErr> {
    let nickname = service::validate_nickname(&req.nickname).map_err(ApiErr::from)?;

    let user_id = Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();

    let conn = db.conn();

    let result = sq_execute(&conn, dbq::users::insert(&user_id, &nickname, &api_key));

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
        let exists: bool = sq_query_row(&conn, dbq::users::email_exists(&email), |row| row.get(0))
            .unwrap_or(false);
        if exists {
            return Err(ApiErr::conflict("email already registered"));
        }
    }

    let user_id = Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();
    let (password_hash, password_salt) =
        crypto::hash_password(&req.password).map_err(ApiErr::from)?;

    {
        let conn = db.conn();
        let result = sq_execute(
            &conn,
            dbq::users::insert_with_email(
                &user_id,
                &nickname,
                &api_key,
                &email,
                &password_hash,
                &password_salt,
            ),
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
    let user = sq_query_row(&conn, dbq::users::get_by_email_for_login(&email), |row| {
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
    let row = sq_query_row(
        &conn,
        dbq::users::lookup_refresh_token(&token_hash),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .map_err(|_| ApiErr::unauthorized("invalid refresh token"))?;

    let (rt_id, user_id, expires_at, nickname) = row;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if expires_at < now {
        sq_execute(&conn, dbq::users::delete_refresh_token_by_id(&rt_id)).ok();
        return Err(ApiErr::unauthorized("refresh token expired"));
    }

    // Rotate: delete old, issue new
    sq_execute(&conn, dbq::users::delete_refresh_token(&token_hash)).ok();
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
    sq_execute(&conn, dbq::users::delete_refresh_token(&token_hash)).ok();
    Ok(Json(OkResponse { ok: true }))
}

/// PUT /api/auth/password — change password (authenticated)
pub async fn change_password(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();
    let (hash, salt): (Option<String>, Option<String>) = sq_query_row(
        &conn,
        dbq::users::get_password_fields(&user.user_id),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
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
    let (new_hash, new_salt) = crypto::hash_password(&req.new_password).map_err(ApiErr::from)?;

    sq_execute(
        &conn,
        dbq::users::update_password(&user.user_id, &new_hash, &new_salt),
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
    let (email, avatar_url): (Option<String>, Option<String>) =
        sq_query_row(&conn, dbq::users::get_email_avatar(&user.user_id), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(ApiErr::from_db("me error"))?;

    // Load linked OAuth providers
    let providers: Vec<oauth::LinkedProvider> =
        sq_query_map(&conn, dbq::oauth::find_by_user(&user.user_id), |row| {
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
        .map_err(ApiErr::from_db("me query oauth"))?;

    // Legacy: first GitHub provider's username
    let github_username = providers
        .iter()
        .find(|p| p.provider == "github")
        .map(|p| p.provider_username.clone());

    let (api_key, created_at): (String, String) = sq_query_row(
        &conn,
        dbq::users::get_settings_fields(&user.user_id),
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap_or_default();

    Ok(Json(UserSettingsResponse {
        user_id: user.user_id,
        nickname: user.nickname,
        api_key,
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
    sq_execute(&conn, dbq::users::update_api_key(&user.user_id, &new_key))
        .map_err(ApiErr::from_db("regenerate key error"))?;

    Ok(Json(RegenerateKeyResponse { api_key: new_key }))
}
