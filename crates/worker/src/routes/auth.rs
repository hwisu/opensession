use opensession_api::{
    crypto, db as dbq,
    oauth::{self, AuthProvidersResponse, OAuthProviderConfig, OAuthProviderInfo},
    service,
    service::AuthToken,
    AuthRegisterRequest, AuthTokenResponse, IssueApiKeyResponse, LoginRequest, LogoutRequest,
    OkResponse, RefreshRequest, UserSettingsResponse, VerifyResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use worker::*;

use crate::config::WorkerConfig;
use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

type ServiceResult<T> = std::result::Result<T, opensession_api::ServiceError>;

#[derive(Debug)]
pub(crate) struct AuthUser {
    pub(crate) user_id: String,
    pub(crate) nickname: String,
    pub(crate) email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserRow {
    id: String,
    nickname: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginRow {
    id: String,
    nickname: String,
    password_hash: Option<String>,
    password_salt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshRow {
    id: String,
    user_id: String,
    expires_at: String,
    nickname: String,
}

#[derive(Debug, Deserialize)]
struct SettingsRow {
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct LegacyApiKeyRow {
    id: String,
    api_key: String,
}

#[derive(Debug, Deserialize)]
struct MigrationMarkerRow {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OAuthIdentityRow {
    provider: String,
    provider_username: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthStateRow {
    provider: String,
    expires_at: String,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthIdentityUserRow {
    user_id: String,
}

fn now_unix() -> u64 {
    let now = chrono::Utc::now().timestamp();
    if now < 0 {
        0
    } else {
        now as u64
    }
}

fn now_sqlite_datetime() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn service_internal(context: &str, err: impl std::fmt::Display) -> opensession_api::ServiceError {
    opensession_api::ServiceError::Internal(format!("{context}: {err}"))
}

fn json_response<T: Serialize>(value: &T, status: u16) -> Result<Response> {
    Response::from_json(value).map(|resp| resp.with_status(status))
}

async fn parse_json<T: for<'de> Deserialize<'de>>(req: &mut Request) -> ServiceResult<T> {
    req.json()
        .await
        .map_err(|_| opensession_api::ServiceError::BadRequest("invalid request body".into()))
}

async fn d1_first<T: for<'de> Deserialize<'de>>(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> ServiceResult<Option<T>> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|e| service_internal(context, e))?;
    stmt.first(None)
        .await
        .map_err(|e| service_internal(context, e))
}

async fn d1_all<T: for<'de> Deserialize<'de>>(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> ServiceResult<Vec<T>> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|e| service_internal(context, e))?;
    let result = stmt.all().await.map_err(|e| service_internal(context, e))?;
    result
        .results::<T>()
        .map_err(|e| service_internal(context, e))
}

async fn d1_run(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> ServiceResult<()> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|e| service_internal(context, e))?;
    let result = stmt.run().await.map_err(|e| service_internal(context, e))?;
    if !result.success() {
        return Err(opensession_api::ServiceError::Internal(
            result
                .error()
                .unwrap_or_else(|| format!("{context} failed")),
        ));
    }
    Ok(())
}

async fn ensure_api_key_backfill(d1: &D1Database) -> ServiceResult<()> {
    const MARKER: &str = "0010_api_keys_hash_backfill_runtime";
    d1_run(
        d1,
        (
            "CREATE TABLE IF NOT EXISTS _runtime_migrations (
                name TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"
            .to_string(),
            sea_query::Values(vec![]),
        ),
        "create runtime migrations table",
    )
    .await?;

    let marker_row: Option<MigrationMarkerRow> = d1_first(
        d1,
        (
            "SELECT name FROM _runtime_migrations WHERE name = ? LIMIT 1".to_string(),
            sea_query::Values(vec![MARKER.into()]),
        ),
        "check api key backfill marker",
    )
    .await?;
    if marker_row.as_ref().is_some_and(|row| row.name == MARKER) {
        return Ok(());
    }

    let legacy_rows: Vec<LegacyApiKeyRow> = d1_all(
        d1,
        (
            "SELECT id, api_key FROM users
             WHERE api_key IS NOT NULL
               AND TRIM(api_key) <> ''
               AND api_key NOT LIKE 'stub:%'
               AND api_key NOT LIKE 'migrated:%'"
                .to_string(),
            sea_query::Values(vec![]),
        ),
        "load legacy api keys",
    )
    .await?;

    for legacy in legacy_rows {
        let key_id = Uuid::new_v4().to_string();
        let key_hash = service::hash_api_key(&legacy.api_key);
        let key_prefix = service::key_prefix(&legacy.api_key);

        d1_run(
            d1,
            dbq::api_keys::insert_active_if_missing(&key_id, &legacy.id, &key_hash, &key_prefix),
            "insert backfilled api key",
        )
        .await?;
        d1_run(
            d1,
            dbq::users::update_api_key(
                &legacy.id,
                &service::generate_api_key_placeholder(&legacy.id),
            ),
            "replace legacy users.api_key with placeholder",
        )
        .await?;
    }

    d1_run(
        d1,
        (
            "INSERT OR IGNORE INTO _runtime_migrations (name) VALUES (?)".to_string(),
            sea_query::Values(vec![MARKER.into()]),
        ),
        "record api key backfill marker",
    )
    .await?;

    Ok(())
}

pub(crate) async fn authenticate(
    req: &Request,
    d1: &D1Database,
    config: &WorkerConfig,
) -> std::result::Result<AuthUser, opensession_api::ServiceError> {
    ensure_api_key_backfill(d1).await?;

    let token = req
        .headers()
        .get("Authorization")
        .map_err(|_| {
            opensession_api::ServiceError::Unauthorized(
                "missing or invalid Authorization header".into(),
            )
        })?
        .and_then(|raw| raw.strip_prefix("Bearer ").map(str::to_owned))
        .ok_or_else(|| {
            opensession_api::ServiceError::Unauthorized(
                "missing or invalid Authorization header".into(),
            )
        })?;

    let resolved = service::resolve_auth_token(&token, &config.jwt_secret, now_unix())?;
    match resolved {
        AuthToken::ApiKey(key) => {
            let key_hash = service::hash_api_key(&key);
            let row: Option<UserRow> = d1_first(
                d1,
                dbq::api_keys::get_user_by_valid_key_hash(&key_hash),
                "lookup user by api key hash",
            )
            .await?;
            let row = row.ok_or_else(|| {
                opensession_api::ServiceError::Unauthorized("invalid API key".into())
            })?;
            Ok(AuthUser {
                user_id: row.id,
                nickname: row.nickname,
                email: row.email,
            })
        }
        AuthToken::Jwt(user_id) => {
            let row: Option<UserRow> =
                d1_first(d1, dbq::users::get_by_id(&user_id), "lookup user by id").await?;
            let row = row.ok_or_else(|| {
                opensession_api::ServiceError::Unauthorized("user not found".into())
            })?;
            Ok(AuthUser {
                user_id: row.id,
                nickname: row.nickname,
                email: row.email,
            })
        }
    }
}

async fn issue_tokens(
    d1: &D1Database,
    config: &WorkerConfig,
    user_id: &str,
    nickname: &str,
) -> ServiceResult<AuthTokenResponse> {
    if config.jwt_secret.is_empty() {
        return Err(opensession_api::ServiceError::Internal(
            "JWT_SECRET not configured".into(),
        ));
    }

    let bundle = service::prepare_token_bundle(&config.jwt_secret, user_id, nickname, now_unix())?;
    d1_run(
        d1,
        dbq::users::insert_refresh_token(
            &bundle.token_id,
            user_id,
            &bundle.token_hash,
            &bundle.expires_at,
        ),
        "insert refresh token",
    )
    .await?;
    Ok(bundle.response)
}

fn find_provider<'a>(
    config: &'a WorkerConfig,
    provider_id: &str,
) -> ServiceResult<&'a OAuthProviderConfig> {
    config
        .oauth_providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| {
            opensession_api::ServiceError::NotFound(format!(
                "OAuth provider '{provider_id}' not found"
            ))
        })
}

fn provider_display_name(provider_id: &str) -> String {
    match provider_id {
        "github" => "GitHub".to_string(),
        "gitlab" => "GitLab".to_string(),
        other => other.to_string(),
    }
}

fn resolve_base_url(req: &Request, config: &WorkerConfig) -> String {
    if let Some(base_url) = config.base_url.as_ref() {
        return base_url.trim_end_matches('/').to_string();
    }

    if let Ok(url) = req.url() {
        let mut base = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
        if let Some(port) = url.port() {
            base.push(':');
            base.push_str(&port.to_string());
        }
        return base;
    }

    "https://opensession.io".to_string()
}

fn redirect_response(location: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Location", location)?;
    Ok(Response::empty()?.with_status(302).with_headers(headers))
}

async fn fetch_text(req: Request, context: &str) -> ServiceResult<(u16, String)> {
    let mut resp = Fetch::Request(req)
        .send()
        .await
        .map_err(|e| service_internal(context, e))?;
    let status = resp.status_code();
    let body = resp
        .text()
        .await
        .map_err(|e| service_internal(context, e))?;
    Ok((status, body))
}

async fn fetch_json(
    url: &str,
    bearer_token: &str,
    context: &str,
) -> ServiceResult<serde_json::Value> {
    let mut init = RequestInit::new();
    init.with_method(Method::Get);
    init.headers
        .set("Authorization", &format!("Bearer {bearer_token}"))
        .map_err(|e| service_internal(context, e))?;
    init.headers
        .set("Accept", "application/json")
        .map_err(|e| service_internal(context, e))?;
    init.headers
        .set("User-Agent", "opensession-worker")
        .map_err(|e| service_internal(context, e))?;

    let req = Request::new_with_init(url, &init).map_err(|e| service_internal(context, e))?;
    let mut resp = Fetch::Request(req)
        .send()
        .await
        .map_err(|e| service_internal(context, e))?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| service_internal(context, e))
}

pub async fn providers(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let payload = AuthProvidersResponse {
        email_password: config.auth_enabled(),
        oauth: config
            .oauth_providers
            .iter()
            .map(|provider| OAuthProviderInfo {
                id: provider.id.clone(),
                display_name: provider.display_name.clone(),
            })
            .collect(),
    };
    json_response(&payload, 200)
}

pub async fn auth_register(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<AuthTokenResponse> = async {
        if config.jwt_secret.is_empty() {
            return Err(opensession_api::ServiceError::Internal(
                "JWT_SECRET not configured".into(),
            ));
        }

        let req: AuthRegisterRequest = parse_json(&mut req).await?;
        let email = service::validate_email(&req.email)?;
        service::validate_password(&req.password)?;
        let nickname = service::validate_nickname(&req.nickname)?;

        let existing: Option<LoginRow> = d1_first(
            &d1,
            dbq::users::get_by_email_for_login(&email),
            "lookup email for register",
        )
        .await?;
        if existing.is_some() {
            return Err(opensession_api::ServiceError::Conflict(
                "email already registered".into(),
            ));
        }

        let user_id = Uuid::new_v4().to_string();
        let api_key_placeholder = service::generate_api_key_placeholder(&user_id);
        let (password_hash, password_salt) = crypto::hash_password(&req.password)?;
        let insert = dbq::users::insert_with_email(
            &user_id,
            &nickname,
            &api_key_placeholder,
            &email,
            &password_hash,
            &password_salt,
        );

        if let Err(err) = d1_run(&d1, insert, "register user").await {
            let msg = err.to_string();
            if msg.contains("UNIQUE constraint failed: users.email") {
                return Err(opensession_api::ServiceError::Conflict(
                    "email already registered".into(),
                ));
            }
            if msg.contains("UNIQUE constraint failed: users.nickname") {
                return Err(opensession_api::ServiceError::Conflict(
                    "nickname already taken".into(),
                ));
            }
            return Err(opensession_api::ServiceError::Internal(
                "internal server error".into(),
            ));
        }

        issue_tokens(&d1, &config, &user_id, &nickname).await
    }
    .await;

    match result {
        Ok(tokens) => json_response(&tokens, 201),
        Err(err) => err.into_err_response(),
    }
}

pub async fn login(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<AuthTokenResponse> = async {
        if config.jwt_secret.is_empty() {
            return Err(opensession_api::ServiceError::Internal(
                "JWT_SECRET not configured".into(),
            ));
        }

        let req: LoginRequest = parse_json(&mut req).await?;
        let email = service::validate_email(&req.email)?;

        let row: Option<LoginRow> = d1_first(
            &d1,
            dbq::users::get_by_email_for_login(&email),
            "lookup user for login",
        )
        .await?;
        let row = row.ok_or_else(|| {
            opensession_api::ServiceError::Unauthorized("invalid email or password".into())
        })?;

        let (hash, salt) = match (row.password_hash, row.password_salt) {
            (Some(hash), Some(salt)) => (hash, salt),
            _ => {
                return Err(opensession_api::ServiceError::Unauthorized(
                    "this account uses OAuth login, not email/password".into(),
                ))
            }
        };

        if !crypto::verify_password(&req.password, &hash, &salt) {
            return Err(opensession_api::ServiceError::Unauthorized(
                "invalid email or password".into(),
            ));
        }

        issue_tokens(&d1, &config, &row.id, &row.nickname).await
    }
    .await;

    match result {
        Ok(tokens) => json_response(&tokens, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn refresh(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<AuthTokenResponse> = async {
        if config.jwt_secret.is_empty() {
            return Err(opensession_api::ServiceError::Internal(
                "JWT_SECRET not configured".into(),
            ));
        }

        let req: RefreshRequest = parse_json(&mut req).await?;
        let token_hash = crypto::hash_token(&req.refresh_token);

        let row: Option<RefreshRow> = d1_first(
            &d1,
            dbq::users::lookup_refresh_token(&token_hash),
            "lookup refresh token",
        )
        .await?;
        let row = row.ok_or_else(|| {
            opensession_api::ServiceError::Unauthorized("invalid refresh token".into())
        })?;

        if row.expires_at < now_sqlite_datetime() {
            let _ = d1_run(
                &d1,
                dbq::users::delete_refresh_token_by_id(&row.id),
                "delete expired refresh token",
            )
            .await;
            return Err(opensession_api::ServiceError::Unauthorized(
                "refresh token expired".into(),
            ));
        }

        let _ = d1_run(
            &d1,
            dbq::users::delete_refresh_token(&token_hash),
            "rotate refresh token",
        )
        .await;

        issue_tokens(&d1, &config, &row.user_id, &row.nickname).await
    }
    .await;

    match result {
        Ok(tokens) => json_response(&tokens, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn logout(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<OkResponse> = async {
        let req: LogoutRequest = parse_json(&mut req).await?;
        let token_hash = crypto::hash_token(&req.refresh_token);
        let _ = d1_run(
            &d1,
            dbq::users::delete_refresh_token(&token_hash),
            "logout refresh token delete",
        )
        .await;
        Ok(OkResponse { ok: true })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn verify(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    match authenticate(&req, &d1, &config).await {
        Ok(user) => json_response(
            &VerifyResponse {
                user_id: user.user_id,
                nickname: user.nickname,
            },
            200,
        ),
        Err(err) => err.into_err_response(),
    }
}

pub async fn me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<UserSettingsResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;

        let settings: Option<SettingsRow> = d1_first(
            &d1,
            dbq::users::get_settings_fields(&user.user_id),
            "load settings fields",
        )
        .await?;
        let settings = settings
            .ok_or_else(|| opensession_api::ServiceError::Unauthorized("user not found".into()))?;

        let identities: Vec<OAuthIdentityRow> = d1_all(
            &d1,
            dbq::oauth::find_by_user(&user.user_id),
            "load oauth identities",
        )
        .await?;

        let oauth_providers = identities
            .iter()
            .map(|identity| oauth::LinkedProvider {
                provider: identity.provider.clone(),
                provider_username: identity.provider_username.clone().unwrap_or_default(),
                display_name: provider_display_name(&identity.provider),
            })
            .collect::<Vec<_>>();

        let avatar_url = identities
            .iter()
            .filter_map(|identity| identity.avatar_url.clone())
            .find(|url| !url.is_empty());

        Ok(UserSettingsResponse {
            user_id: user.user_id,
            nickname: user.nickname,
            created_at: settings.created_at,
            email: user.email,
            avatar_url,
            oauth_providers,
        })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn issue_api_key(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<IssueApiKeyResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;
        let now = now_unix();
        let grace_until = service::grace_until_sqlite(now)?;
        let key = service::generate_api_key();
        let key_hash = service::hash_api_key(&key);
        let key_prefix = service::key_prefix(&key);
        let key_id = Uuid::new_v4().to_string();

        d1_run(
            &d1,
            dbq::api_keys::move_active_to_grace(&user.user_id, &grace_until),
            "move active keys to grace",
        )
        .await?;
        d1_run(
            &d1,
            dbq::api_keys::insert_active(&key_id, &user.user_id, &key_hash, &key_prefix),
            "insert active api key",
        )
        .await?;

        Ok(IssueApiKeyResponse { api_key: key })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn oauth_redirect(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<String> = async {
        if config.jwt_secret.is_empty() {
            return Err(opensession_api::ServiceError::Internal(
                "JWT_SECRET not configured".into(),
            ));
        }

        let provider_id = ctx
            .param("provider")
            .ok_or_else(|| opensession_api::ServiceError::BadRequest("missing provider".into()))?;
        let provider = find_provider(&config, provider_id)?;

        let state = crypto::generate_token()?;
        let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        d1_run(
            &d1,
            dbq::oauth::insert_state(&state, provider_id, &expires_at, None),
            "insert oauth state",
        )
        .await?;

        let base_url = resolve_base_url(&req, &config);
        let redirect_uri = format!("{base_url}/api/auth/oauth/{provider_id}/callback");
        Ok(oauth::build_authorize_url(provider, &redirect_uri, &state))
    }
    .await;

    match result {
        Ok(location) => redirect_response(&location),
        Err(err) => err.into_err_response(),
    }
}

pub async fn oauth_callback(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<String> = async {
        let provider_id = ctx
            .param("provider")
            .ok_or_else(|| opensession_api::ServiceError::BadRequest("missing provider".into()))?
            .to_string();
        let provider = find_provider(&config, &provider_id)?;
        let base_url = resolve_base_url(&req, &config);

        let query: std::collections::HashMap<String, String> = req
            .url()
            .map_err(|e| service_internal("parse callback url", e))?
            .query_pairs()
            .into_owned()
            .collect();
        let code = query.get("code").cloned().ok_or_else(|| {
            opensession_api::ServiceError::BadRequest("missing code parameter".into())
        })?;
        let state_param = query.get("state").cloned().ok_or_else(|| {
            opensession_api::ServiceError::BadRequest("missing state parameter".into())
        })?;

        let state_row: Option<OAuthStateRow> = d1_first(
            &d1,
            dbq::oauth::validate_state(&state_param),
            "validate oauth state",
        )
        .await?;
        let state_row = state_row.ok_or_else(|| {
            opensession_api::ServiceError::BadRequest("invalid OAuth state".into())
        })?;

        if state_row.provider != provider_id {
            return Err(opensession_api::ServiceError::BadRequest(
                "OAuth state provider mismatch".into(),
            ));
        }
        if state_row.expires_at < now_sqlite_datetime() {
            return Err(opensession_api::ServiceError::BadRequest(
                "OAuth state expired".into(),
            ));
        }
        if state_row.user_id.is_some() {
            return Err(opensession_api::ServiceError::BadRequest(
                "OAuth linking callback is not supported in worker".into(),
            ));
        }

        let _ = d1_run(
            &d1,
            dbq::oauth::delete_state(&state_param),
            "delete oauth state",
        )
        .await;

        let redirect_uri = format!("{base_url}/api/auth/oauth/{provider_id}/callback");
        let token_body = oauth::build_token_request_form_encoded(provider, &code, &redirect_uri);
        let mut token_init = RequestInit::new();
        token_init.with_method(Method::Post);
        token_init
            .headers
            .set("Accept", "application/json")
            .map_err(|e| service_internal("oauth token request headers", e))?;
        token_init
            .headers
            .set("Content-Type", "application/x-www-form-urlencoded")
            .map_err(|e| service_internal("oauth token request headers", e))?;
        token_init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(&token_body)));

        let token_req = Request::new_with_init(&provider.token_url, &token_init)
            .map_err(|e| service_internal("oauth token request build", e))?;
        let (token_status, token_raw) = fetch_text(token_req, "oauth token exchange").await?;

        let access_token = oauth::parse_access_token_response(&token_raw).map_err(|e| {
            let mut msg = if token_status < 400 {
                e.message().to_string()
            } else {
                format!("{} (status {token_status})", e.message())
            };
            if msg.contains("incorrect_client_credentials") {
                msg.push_str(
                    "; verify GITHUB_CLIENT_ID/GITHUB_CLIENT_SECRET match the GitHub OAuth app",
                );
            }
            opensession_api::ServiceError::Internal(msg)
        })?;

        let userinfo = fetch_json(
            &provider.userinfo_url,
            &access_token,
            "fetch oauth userinfo",
        )
        .await?;
        let emails = match provider.email_url.as_ref() {
            Some(email_url) => {
                let result = fetch_json(email_url, &access_token, "fetch oauth emails").await;
                result
                    .ok()
                    .and_then(|json| json.as_array().map(|arr| arr.to_vec()))
            }
            None => None,
        };

        let user_info = oauth::extract_user_info(provider, &userinfo, emails.as_deref())?;

        let existing_by_provider: Option<OAuthIdentityUserRow> = d1_first(
            &d1,
            dbq::oauth::find_by_provider(&provider_id, &user_info.provider_user_id),
            "lookup oauth identity",
        )
        .await?;

        let (user_id, nickname) = if let Some(existing) = existing_by_provider {
            let existing_user: Option<UserRow> = d1_first(
                &d1,
                dbq::users::get_by_id(&existing.user_id),
                "lookup oauth user",
            )
            .await?;
            let existing_user = existing_user.ok_or_else(|| {
                opensession_api::ServiceError::Unauthorized("user not found".into())
            })?;

            let _ = d1_run(
                &d1,
                dbq::oauth::upsert_identity(
                    &existing.user_id,
                    &provider_id,
                    &user_info.provider_user_id,
                    Some(&user_info.username),
                    user_info.avatar_url.as_deref(),
                    None,
                ),
                "update oauth identity",
            )
            .await;
            (existing.user_id, existing_user.nickname)
        } else {
            let existing_by_email = match user_info.email.as_deref() {
                Some(email) => {
                    d1_first::<LoginRow>(
                        &d1,
                        dbq::users::get_by_email_for_login(email),
                        "lookup user by oauth email",
                    )
                    .await?
                }
                None => None,
            };

            if let Some(existing) = existing_by_email {
                let _ = d1_run(
                    &d1,
                    dbq::oauth::upsert_identity(
                        &existing.id,
                        &provider_id,
                        &user_info.provider_user_id,
                        Some(&user_info.username),
                        user_info.avatar_url.as_deref(),
                        None,
                    ),
                    "link oauth identity",
                )
                .await;
                (existing.id, existing.nickname)
            } else {
                let user_id = Uuid::new_v4().to_string();
                let nickname = user_info.username.clone();
                let api_key_placeholder = service::generate_api_key_placeholder(&user_id);

                d1_run(
                    &d1,
                    dbq::users::insert_oauth(
                        &user_id,
                        &nickname,
                        &api_key_placeholder,
                        user_info.email.as_deref(),
                    ),
                    "insert oauth user",
                )
                .await?;

                d1_run(
                    &d1,
                    dbq::oauth::upsert_identity(
                        &user_id,
                        &provider_id,
                        &user_info.provider_user_id,
                        Some(&user_info.username),
                        user_info.avatar_url.as_deref(),
                        None,
                    ),
                    "insert oauth identity",
                )
                .await?;

                (user_id, nickname)
            }
        };

        let tokens = issue_tokens(&d1, &config, &user_id, &nickname).await?;
        Ok(format!(
            "{base_url}/auth/callback#access_token={}&refresh_token={}&expires_in={}",
            tokens.access_token, tokens.refresh_token, tokens.expires_in
        ))
    }
    .await;

    match result {
        Ok(location) => redirect_response(&location),
        Err(err) => err.into_err_response(),
    }
}
