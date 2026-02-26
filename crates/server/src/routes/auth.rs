use axum::{
    extract::{FromRef, FromRequestParts, Path, State},
    http::{request::Parts, HeaderMap, StatusCode},
    Json,
};
use uuid::Uuid;

use opensession_api::{
    crypto, db as dbq, oauth, service, service::AuthToken, AuthRegisterRequest, AuthTokenResponse,
    ChangePasswordRequest, CreateGitCredentialRequest, GitCredentialSummary, IssueApiKeyResponse,
    ListGitCredentialsResponse, LoginRequest, LogoutRequest, OkResponse, RefreshRequest,
    UserSettingsResponse, VerifyResponse,
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
/// 1. `osk_` prefix → API key hash DB lookup
/// 2. Otherwise → JWT verify → user_id DB lookup
pub struct AuthUser {
    pub user_id: String,
    pub nickname: String,
    #[allow(dead_code)]
    pub email: Option<String>,
}

fn resolve_auth_user(token: &str, db: &Db, config: &AppConfig) -> Result<AuthUser, ApiErr> {
    let now = chrono::Utc::now().timestamp() as u64;
    let resolved = service::resolve_auth_token(token, &config.jwt_secret, now)
        .map_err(|e| ApiErr::unauthorized(e.message()))?;

    let conn = db.conn();
    match resolved {
        AuthToken::ApiKey(key) => {
            let key_hash = service::hash_api_key(&key);
            sq_query_row(
                &conn,
                dbq::api_keys::get_user_by_valid_key_hash(&key_hash),
                |row| {
                    Ok(AuthUser {
                        user_id: row.get(0)?,
                        nickname: row.get(1)?,
                        email: row.get(2)?,
                    })
                },
            )
            .map_err(|_| ApiErr::unauthorized("invalid API key"))
        }
        AuthToken::Jwt(user_id) => sq_query_row(&conn, dbq::users::get_by_id(&user_id), |row| {
            Ok(AuthUser {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                email: row.get(2)?,
            })
        })
        .map_err(|_| ApiErr::unauthorized("user not found")),
    }
}

pub fn try_auth_from_headers(
    headers: &HeaderMap,
    db: &Db,
    config: &AppConfig,
) -> Result<Option<AuthUser>, ApiErr> {
    let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return Ok(None);
    };

    resolve_auth_user(token, db, config).map(Some)
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
        resolve_auth_user(token, &db, &config)
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
    let (password_hash, password_salt) =
        crypto::hash_password(&req.password).map_err(ApiErr::from)?;

    {
        let conn = db.conn();
        let result = sq_execute(
            &conn,
            dbq::users::insert_with_email(
                &user_id,
                &nickname,
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

    let created_at: String = sq_query_row(
        &conn,
        dbq::users::get_settings_fields(&user.user_id),
        |row| row.get(0),
    )
    .unwrap_or_default();

    Ok(Json(UserSettingsResponse {
        user_id: user.user_id,
        nickname: user.nickname,
        created_at,
        email,
        avatar_url,
        oauth_providers: providers,
    }))
}

// ---------------------------------------------------------------------------
// Issue API key
// ---------------------------------------------------------------------------

/// POST /api/auth/api-keys/issue — issue a new API key.
///
/// The new key is visible only in this response.
/// Previously active keys are moved to grace mode for a limited period.
pub async fn issue_api_key(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<IssueApiKeyResponse>, ApiErr> {
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    let grace_until = service::grace_until_sqlite(now).map_err(ApiErr::from)?;
    let new_key = service::generate_api_key();
    let key_hash = service::hash_api_key(&new_key);
    let key_prefix = service::key_prefix(&new_key);
    let key_id = Uuid::new_v4().to_string();

    let conn = db.conn();
    sq_execute(
        &conn,
        dbq::api_keys::move_active_to_grace(&user.user_id, &grace_until),
    )
    .map_err(ApiErr::from_db("issue api key move old keys"))?;
    sq_execute(
        &conn,
        dbq::api_keys::insert_active(&key_id, &user.user_id, &key_hash, &key_prefix),
    )
    .map_err(ApiErr::from_db("issue api key insert"))?;

    Ok(Json(IssueApiKeyResponse { api_key: new_key }))
}

fn normalize_header_name(raw: &str) -> Result<String, ApiErr> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiErr::bad_request("header_name is required"));
    }
    if trimmed.len() > 64 {
        return Err(ApiErr::bad_request(
            "header_name is too long (max 64 chars)",
        ));
    }
    if !trimmed.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
            )
    }) {
        return Err(ApiErr::bad_request(
            "header_name contains invalid characters",
        ));
    }
    Ok(trimmed.to_string())
}

fn normalize_host(raw: &str) -> Result<String, ApiErr> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ApiErr::bad_request("host is required"));
    }
    if trimmed.len() > 255 {
        return Err(ApiErr::bad_request("host is too long (max 255 chars)"));
    }
    if trimmed.contains('/') || trimmed.contains(' ') {
        return Err(ApiErr::bad_request(
            "host must not contain path separators or spaces",
        ));
    }
    if trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return Ok(trimmed);
    }
    Err(ApiErr::bad_request("host contains invalid characters"))
}

fn normalize_path_prefix(raw: Option<&str>) -> Result<String, ApiErr> {
    let trimmed = raw.unwrap_or_default().trim().trim_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.len() > 512 {
        return Err(ApiErr::bad_request(
            "path_prefix is too long (max 512 chars)",
        ));
    }

    let mut segments = Vec::<String>::new();
    for part in trimmed.split('/') {
        let seg = part.trim();
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') {
            return Err(ApiErr::bad_request("path_prefix contains invalid segments"));
        }
        segments.push(seg.to_string());
    }
    if let Some(last) = segments.last_mut() {
        *last = last.strip_suffix(".git").unwrap_or(last).to_string();
    }
    Ok(segments.join("/"))
}

/// GET /api/auth/git-credentials — list masked git credentials for authenticated user.
pub async fn list_git_credentials(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<ListGitCredentialsResponse>, ApiErr> {
    let conn = db.conn();
    let credentials = sq_query_map(
        &conn,
        dbq::git_credentials::list_by_user(&user.user_id),
        |row| {
            Ok(GitCredentialSummary {
                id: row.get(0)?,
                label: row.get(1)?,
                host: row.get(2)?,
                path_prefix: row.get(3)?,
                header_name: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                last_used_at: row.get(7)?,
            })
        },
    )
    .map_err(ApiErr::from_db("list git credentials"))?;

    Ok(Json(ListGitCredentialsResponse { credentials }))
}

/// POST /api/auth/git-credentials — register a user-managed git credential.
pub async fn create_git_credential(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    user: AuthUser,
    Json(req): Json<CreateGitCredentialRequest>,
) -> Result<(StatusCode, Json<GitCredentialSummary>), ApiErr> {
    let keyring = config
        .credential_keyring
        .as_ref()
        .ok_or_else(|| ApiErr::internal("credential encryption is not configured"))?;

    let label = req.label.trim().to_string();
    if label.is_empty() {
        return Err(ApiErr::bad_request("label is required"));
    }
    if label.len() > 120 {
        return Err(ApiErr::bad_request("label is too long (max 120 chars)"));
    }
    let host = normalize_host(&req.host)?;
    let path_prefix = normalize_path_prefix(req.path_prefix.as_deref())?;
    let header_name = normalize_header_name(&req.header_name)?;
    let header_value = req.header_value.trim();
    if header_value.is_empty() {
        return Err(ApiErr::bad_request("header_value is required"));
    }
    let header_value_enc = keyring.encrypt(header_value).map_err(ApiErr::from)?;

    let id = Uuid::new_v4().to_string();
    let conn = db.conn();
    sq_execute(
        &conn,
        dbq::git_credentials::insert(
            &id,
            &user.user_id,
            &label,
            &host,
            &path_prefix,
            &header_name,
            &header_value_enc,
        ),
    )
    .map_err(ApiErr::from_db("create git credential"))?;

    let created = sq_query_row(
        &conn,
        dbq::git_credentials::get_by_id_and_user(&id, &user.user_id),
        |row| {
            let current_id: String = row.get(0)?;
            Ok(GitCredentialSummary {
                id: current_id,
                label: row.get(1)?,
                host: row.get(2)?,
                path_prefix: row.get(3)?,
                header_name: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                last_used_at: row.get(7)?,
            })
        },
    )
    .map_err(ApiErr::from_db("reload git credential"))?;

    Ok((StatusCode::CREATED, Json(created)))
}

/// DELETE /api/auth/git-credentials/:id — remove a user-managed git credential.
pub async fn delete_git_credential(
    Path(id): Path<String>,
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();
    let affected = sq_execute(
        &conn,
        dbq::git_credentials::delete_by_id_and_user(id.as_str(), &user.user_id),
    )
    .map_err(ApiErr::from_db("delete git credential"))?;

    if affected == 0 {
        return Err(ApiErr::not_found("credential not found"));
    }
    Ok(Json(OkResponse { ok: true }))
}

#[cfg(test)]
mod tests {
    use super::{normalize_header_name, normalize_host, normalize_path_prefix};

    #[test]
    fn normalize_host_accepts_valid_and_rejects_invalid() {
        assert_eq!(
            normalize_host("GitLab.INTERNAL.example.com").unwrap_or_else(|_| panic!("valid host")),
            "gitlab.internal.example.com"
        );
        assert!(normalize_host("bad host/path").is_err());
        assert!(normalize_host("").is_err());
    }

    #[test]
    fn normalize_path_prefix_trims_and_strips_git_suffix() {
        assert_eq!(
            normalize_path_prefix(Some("/group/sub/repo.git/"))
                .unwrap_or_else(|_| panic!("prefix")),
            "group/sub/repo"
        );
        assert_eq!(
            normalize_path_prefix(None).unwrap_or_else(|_| panic!("empty")),
            ""
        );
        assert!(normalize_path_prefix(Some("../bad")).is_err());
    }

    #[test]
    fn normalize_header_name_enforces_token_chars() {
        assert_eq!(
            normalize_header_name("X-GitLab-Token").unwrap_or_else(|_| panic!("header")),
            "X-GitLab-Token"
        );
        assert!(normalize_header_name("bad header").is_err());
    }
}
