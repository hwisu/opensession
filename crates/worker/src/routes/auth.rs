use opensession_api::{
    crypto, db as dbq,
    oauth::{self, AuthProvidersResponse, OAuthProviderConfig, OAuthProviderInfo},
    service,
    service::AuthToken,
    AuthRegisterRequest, AuthTokenResponse, CreateGitCredentialRequest, GitCredentialSummary,
    IssueApiKeyResponse, ListGitCredentialsResponse, LoginRequest, LogoutRequest, OkResponse,
    RefreshRequest, ServiceError, UserSettingsResponse, VerifyResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use worker::*;

use crate::config::WorkerConfig;
use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

type ServiceResult<T> = std::result::Result<T, opensession_api::ServiceError>;

const ACCESS_COOKIE_NAME: &str = "opensession_access_token";
const REFRESH_COOKIE_NAME: &str = "opensession_refresh_token";
const CSRF_COOKIE_NAME: &str = "opensession_csrf_token";
const CSRF_HEADER_NAME: &str = "x-csrf-token";

#[derive(Debug)]
pub(crate) struct AuthUser {
    pub(crate) user_id: String,
    pub(crate) nickname: String,
    pub(crate) auth_via_cookie: bool,
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

#[derive(Debug, Deserialize)]
struct GitCredentialSummaryRow {
    id: String,
    label: String,
    host: String,
    path_prefix: String,
    header_name: String,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
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

fn parse_cookie_value(headers: &Headers, name: &str) -> Option<String> {
    headers.get("Cookie").ok().flatten().and_then(|raw| {
        raw.split(';').find_map(|entry| {
            let mut parts = entry.trim().splitn(2, '=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();
            if key == name {
                Some(value.to_string())
            } else {
                None
            }
        })
    })
}

fn constant_time_eq(lhs: &str, rhs: &str) -> bool {
    let left = lhs.as_bytes();
    let right = rhs.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn secure_cookie_mode(req: &Request, config: &WorkerConfig) -> bool {
    if let Ok(Some(proto)) = req.headers().get("x-forwarded-proto") {
        if proto
            .split(',')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("https"))
        {
            return true;
        }
    }
    if let Ok(url) = req.url() {
        if url.scheme().eq_ignore_ascii_case("https") {
            return true;
        }
    }
    config
        .base_url
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("https://"))
}

fn build_set_cookie(
    name: &str,
    value: &str,
    max_age_secs: i64,
    path: &str,
    http_only: bool,
    secure: bool,
) -> String {
    let mut cookie = format!("{name}={value}; Path={path}; Max-Age={max_age_secs}; SameSite=Lax");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn auth_cookie_values(
    req: &Request,
    config: &WorkerConfig,
    tokens: &AuthTokenResponse,
) -> ServiceResult<Vec<String>> {
    let secure = secure_cookie_mode(req, config);
    let csrf = crypto::generate_token()?;
    Ok(vec![
        build_set_cookie(
            ACCESS_COOKIE_NAME,
            &tokens.access_token,
            tokens.expires_in as i64,
            "/api",
            true,
            secure,
        ),
        build_set_cookie(
            REFRESH_COOKIE_NAME,
            &tokens.refresh_token,
            crypto::REFRESH_EXPIRY_SECS as i64,
            "/api",
            true,
            secure,
        ),
        build_set_cookie(
            CSRF_COOKIE_NAME,
            &csrf,
            crypto::REFRESH_EXPIRY_SECS as i64,
            "/",
            false,
            secure,
        ),
    ])
}

fn clear_auth_cookie_values(req: &Request, config: &WorkerConfig) -> Vec<String> {
    let secure = secure_cookie_mode(req, config);
    vec![
        build_set_cookie(ACCESS_COOKIE_NAME, "", 0, "/api", true, secure),
        build_set_cookie(REFRESH_COOKIE_NAME, "", 0, "/api", true, secure),
        build_set_cookie(CSRF_COOKIE_NAME, "", 0, "/", false, secure),
    ]
}

fn enforce_csrf_if_cookie_auth(
    req: &Request,
    config: &WorkerConfig,
    using_cookie_auth: bool,
) -> ServiceResult<()> {
    if !using_cookie_auth {
        return Ok(());
    }
    let origin = req
        .headers()
        .get("Origin")
        .map_err(|e| service_internal("read origin header", e))?
        .ok_or_else(|| {
            opensession_api::ServiceError::Unauthorized("missing request origin".into())
        })?;
    if !config.allowed_origins.iter().any(|value| value == &origin) {
        return Err(opensession_api::ServiceError::Unauthorized(
            "request origin is not allowed".into(),
        ));
    }
    let csrf_cookie = parse_cookie_value(req.headers(), CSRF_COOKIE_NAME)
        .ok_or_else(|| opensession_api::ServiceError::Unauthorized("missing csrf cookie".into()))?;
    let csrf_header = req
        .headers()
        .get(CSRF_HEADER_NAME)
        .map_err(|e| service_internal("read csrf header", e))?
        .ok_or_else(|| opensession_api::ServiceError::Unauthorized("missing csrf header".into()))?;
    if !constant_time_eq(&csrf_cookie, &csrf_header) {
        return Err(opensession_api::ServiceError::Unauthorized(
            "csrf token mismatch".into(),
        ));
    }
    Ok(())
}

fn service_internal(context: &str, err: impl std::fmt::Display) -> opensession_api::ServiceError {
    opensession_api::ServiceError::Internal(format!("{context}: {err}"))
}

fn json_response<T: Serialize>(value: &T, status: u16) -> Result<Response> {
    Response::from_json(value).map(|resp| resp.with_status(status))
}

fn json_response_with_cookies<T: Serialize>(
    value: &T,
    status: u16,
    cookies: &[String],
) -> Result<Response> {
    let resp = json_response(value, status)?;
    let headers = Headers::new();
    for cookie in cookies {
        headers.append("Set-Cookie", cookie)?;
    }
    Ok(resp.with_headers(headers))
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

pub(crate) async fn authenticate(
    req: &Request,
    d1: &D1Database,
    config: &WorkerConfig,
) -> std::result::Result<AuthUser, opensession_api::ServiceError> {
    let header_token = req
        .headers()
        .get("Authorization")
        .map_err(|_| {
            opensession_api::ServiceError::Unauthorized(
                "missing or invalid Authorization header".into(),
            )
        })?
        .and_then(|raw| raw.strip_prefix("Bearer ").map(str::to_owned));
    let cookie_token = parse_cookie_value(req.headers(), ACCESS_COOKIE_NAME);
    let (token, auth_via_cookie) = if let Some(token) = header_token {
        (token, false)
    } else if let Some(token) = cookie_token {
        (token, true)
    } else {
        return Err(opensession_api::ServiceError::Unauthorized(
            "missing authentication token".into(),
        ));
    };

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
                auth_via_cookie,
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
                auth_via_cookie,
                email: row.email,
            })
        }
    }
}

pub(crate) async fn authenticate_optional(
    req: &Request,
    d1: &D1Database,
    config: &WorkerConfig,
) -> std::result::Result<Option<AuthUser>, opensession_api::ServiceError> {
    let header = req.headers().get("Authorization").map_err(|_| {
        opensession_api::ServiceError::Unauthorized(
            "missing or invalid Authorization header".into(),
        )
    })?;
    if let Some(raw) = header {
        if !raw.starts_with("Bearer ") {
            return Err(opensession_api::ServiceError::Unauthorized(
                "missing or invalid Authorization header".into(),
            ));
        }
        return authenticate(req, d1, config).await.map(Some);
    }
    if parse_cookie_value(req.headers(), ACCESS_COOKIE_NAME).is_some() {
        return authenticate(req, d1, config).await.map(Some);
    }
    Ok(None)
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

fn oauth_provider_host(provider: &OAuthProviderConfig) -> ServiceResult<String> {
    let parsed = Url::parse(&provider.token_url).map_err(|_| {
        opensession_api::ServiceError::Internal("invalid OAuth provider token URL".into())
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        opensession_api::ServiceError::Internal("OAuth provider token URL missing host".into())
    })?;
    Ok(host.to_ascii_lowercase())
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

fn redirect_response_with_cookies(location: &str, cookies: &[String]) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Location", location)?;
    for cookie in cookies {
        headers.append("Set-Cookie", cookie)?;
    }
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
        let (password_hash, password_salt) = crypto::hash_password(&req.password)?;
        let insert = dbq::users::insert_with_email(
            &user_id,
            &nickname,
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
        Ok(tokens) => match auth_cookie_values(&req, &config, &tokens) {
            Ok(cookies) => json_response_with_cookies(&tokens, 201, &cookies),
            Err(err) => err.into_err_response(),
        },
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
        Ok(tokens) => match auth_cookie_values(&req, &config, &tokens) {
            Ok(cookies) => json_response_with_cookies(&tokens, 200, &cookies),
            Err(err) => err.into_err_response(),
        },
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

        let body_text = req
            .text()
            .await
            .map_err(|e| service_internal("read refresh body", e))?;
        let body_token = if body_text.trim().is_empty() {
            None
        } else {
            let payload: RefreshRequest = serde_json::from_str(&body_text).map_err(|_| {
                opensession_api::ServiceError::BadRequest("invalid request body".into())
            })?;
            let token = payload.refresh_token.trim().to_string();
            if token.is_empty() {
                return Err(opensession_api::ServiceError::BadRequest(
                    "refresh_token is required".into(),
                ));
            }
            Some(token)
        };
        let cookie_token = parse_cookie_value(req.headers(), REFRESH_COOKIE_NAME);
        let (refresh_token, using_cookie_refresh) = match (body_token, cookie_token) {
            (Some(token), _) => (token, false),
            (None, Some(token)) => (token, true),
            (None, None) => {
                return Err(opensession_api::ServiceError::Unauthorized(
                    "missing refresh token".into(),
                ));
            }
        };
        enforce_csrf_if_cookie_auth(&req, &config, using_cookie_refresh)?;
        let token_hash = crypto::hash_token(&refresh_token);

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
        Ok(tokens) => match auth_cookie_values(&req, &config, &tokens) {
            Ok(cookies) => json_response_with_cookies(&tokens, 200, &cookies),
            Err(err) => err.into_err_response(),
        },
        Err(err) => err.into_err_response(),
    }
}

pub async fn logout(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<OkResponse> = async {
        let body_text = req
            .text()
            .await
            .map_err(|e| service_internal("read logout body", e))?;
        let body_token = if body_text.trim().is_empty() {
            None
        } else {
            let payload: LogoutRequest = serde_json::from_str(&body_text).map_err(|_| {
                opensession_api::ServiceError::BadRequest("invalid request body".into())
            })?;
            let token = payload.refresh_token.trim().to_string();
            if token.is_empty() {
                return Err(opensession_api::ServiceError::BadRequest(
                    "refresh_token is required".into(),
                ));
            }
            Some(token)
        };
        let cookie_token = parse_cookie_value(req.headers(), REFRESH_COOKIE_NAME);
        let token_and_source = match (body_token, cookie_token) {
            (Some(token), _) => Some((token, false)),
            (None, Some(token)) => Some((token, true)),
            (None, None) => None,
        };
        if let Some((refresh_token, using_cookie_refresh)) = token_and_source {
            enforce_csrf_if_cookie_auth(&req, &config, using_cookie_refresh)?;
            let token_hash = crypto::hash_token(&refresh_token);
            let _ = d1_run(
                &d1,
                dbq::users::delete_refresh_token(&token_hash),
                "logout refresh token delete",
            )
            .await;
        }
        Ok(OkResponse { ok: true })
    }
    .await;

    match result {
        Ok(body) => {
            let cookies = clear_auth_cookie_values(&req, &config);
            json_response_with_cookies(&body, 200, &cookies)
        }
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
        enforce_csrf_if_cookie_auth(&req, &config, user.auth_via_cookie)?;
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

fn normalize_header_name(raw: &str) -> ServiceResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::BadRequest("header_name is required".into()));
    }
    if trimmed.len() > 64 {
        return Err(ServiceError::BadRequest(
            "header_name is too long (max 64 chars)".into(),
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
        return Err(ServiceError::BadRequest(
            "header_name contains invalid characters".into(),
        ));
    }
    Ok(trimmed.to_string())
}

fn normalize_host(raw: &str) -> ServiceResult<String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(ServiceError::BadRequest("host is required".into()));
    }
    if trimmed.len() > 255 {
        return Err(ServiceError::BadRequest(
            "host is too long (max 255 chars)".into(),
        ));
    }
    if trimmed.contains('/') || trimmed.contains(' ') {
        return Err(ServiceError::BadRequest(
            "host must not contain path separators or spaces".into(),
        ));
    }
    if trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return Ok(trimmed);
    }
    Err(ServiceError::BadRequest(
        "host contains invalid characters".into(),
    ))
}

fn normalize_path_prefix(raw: Option<&str>) -> ServiceResult<String> {
    let trimmed = raw.unwrap_or_default().trim().trim_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.len() > 512 {
        return Err(ServiceError::BadRequest(
            "path_prefix is too long (max 512 chars)".into(),
        ));
    }
    let mut segments = Vec::<String>::new();
    for part in trimmed.split('/') {
        let seg = part.trim();
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') {
            return Err(ServiceError::BadRequest(
                "path_prefix contains invalid segments".into(),
            ));
        }
        segments.push(seg.to_string());
    }
    if let Some(last) = segments.last_mut() {
        *last = last.strip_suffix(".git").unwrap_or(last).to_string();
    }
    Ok(segments.join("/"))
}

pub async fn list_git_credentials(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<ListGitCredentialsResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;
        let rows: Vec<GitCredentialSummaryRow> = d1_all(
            &d1,
            dbq::git_credentials::list_by_user(&user.user_id),
            "list git credentials",
        )
        .await?;
        let credentials = rows
            .into_iter()
            .map(|row| GitCredentialSummary {
                id: row.id,
                label: row.label,
                host: row.host,
                path_prefix: row.path_prefix,
                header_name: row.header_name,
                created_at: row.created_at,
                updated_at: row.updated_at,
                last_used_at: row.last_used_at,
            })
            .collect();
        Ok(ListGitCredentialsResponse { credentials })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 200),
        Err(err) => err.into_err_response(),
    }
}

pub async fn create_git_credential(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<GitCredentialSummary> = async {
        let user = authenticate(&req, &d1, &config).await?;
        enforce_csrf_if_cookie_auth(&req, &config, user.auth_via_cookie)?;
        let payload: CreateGitCredentialRequest = parse_json(&mut req).await?;

        let keyring = config.credential_keyring.as_ref().ok_or_else(|| {
            ServiceError::Internal("credential encryption is not configured".into())
        })?;

        let label = payload.label.trim().to_string();
        if label.is_empty() {
            return Err(ServiceError::BadRequest("label is required".into()));
        }
        if label.len() > 120 {
            return Err(ServiceError::BadRequest(
                "label is too long (max 120 chars)".into(),
            ));
        }
        let host = normalize_host(&payload.host)?;
        let path_prefix = normalize_path_prefix(payload.path_prefix.as_deref())?;
        let header_name = normalize_header_name(&payload.header_name)?;
        let header_value = payload.header_value.trim();
        if header_value.is_empty() {
            return Err(ServiceError::BadRequest("header_value is required".into()));
        }
        let header_value_enc = keyring.encrypt(header_value)?;

        let credential_id = Uuid::new_v4().to_string();
        d1_run(
            &d1,
            dbq::git_credentials::insert(
                &credential_id,
                &user.user_id,
                &label,
                &host,
                &path_prefix,
                &header_name,
                &header_value_enc,
            ),
            "insert git credential",
        )
        .await?;

        let row = d1_first::<GitCredentialSummaryRow>(
            &d1,
            dbq::git_credentials::get_by_id_and_user(&credential_id, &user.user_id),
            "reload git credential",
        )
        .await?
        .ok_or_else(|| ServiceError::Internal("failed to reload git credential".into()))?;

        Ok(GitCredentialSummary {
            id: row.id,
            label: row.label,
            host: row.host,
            path_prefix: row.path_prefix,
            header_name: row.header_name,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_used_at: row.last_used_at,
        })
    }
    .await;

    match result {
        Ok(body) => json_response(&body, 201),
        Err(err) => err.into_err_response(),
    }
}

pub async fn delete_git_credential(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = match storage::get_d1(&ctx.env) {
        Ok(db) => db,
        Err(err) => return service_internal("load d1 binding", err).into_err_response(),
    };

    let result: ServiceResult<OkResponse> = async {
        let user = authenticate(&req, &d1, &config).await?;
        enforce_csrf_if_cookie_auth(&req, &config, user.auth_via_cookie)?;
        let id = ctx
            .param("id")
            .ok_or_else(|| ServiceError::BadRequest("missing credential id".into()))?;

        let existing = d1_first::<GitCredentialSummaryRow>(
            &d1,
            dbq::git_credentials::get_by_id_and_user(id, &user.user_id),
            "lookup git credential",
        )
        .await?;
        if existing.is_none() {
            return Err(ServiceError::NotFound("credential not found".into()));
        }

        d1_run(
            &d1,
            dbq::git_credentials::delete_by_id_and_user(id, &user.user_id),
            "delete git credential",
        )
        .await?;
        Ok(OkResponse { ok: true })
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

    let result: ServiceResult<(String, Vec<String>)> = async {
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

                d1_run(
                    &d1,
                    dbq::users::insert_oauth(&user_id, &nickname, user_info.email.as_deref()),
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

        if let Some(keyring) = config.credential_keyring.as_ref() {
            let token_id = Uuid::new_v4().to_string();
            let access_token_enc = keyring.encrypt(&access_token)?;
            d1_run(
                &d1,
                dbq::oauth_provider_tokens::upsert_access_token(
                    &token_id,
                    &user_id,
                    &provider_id,
                    &oauth_provider_host(provider)?,
                    &access_token_enc,
                    None,
                ),
                "upsert oauth provider token",
            )
            .await?;
        }

        let tokens = issue_tokens(&d1, &config, &user_id, &nickname).await?;
        let cookies = auth_cookie_values(&req, &config, &tokens)?;
        Ok((format!("{base_url}/auth/callback"), cookies))
    }
    .await;

    match result {
        Ok((location, cookies)) => redirect_response_with_cookies(&location, &cookies),
        Err(err) => err.into_err_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_header_name, normalize_host, normalize_path_prefix};

    #[test]
    fn normalize_host_accepts_valid_and_rejects_invalid() {
        assert_eq!(
            normalize_host("GitLab.INTERNAL.example.com").expect("valid host"),
            "gitlab.internal.example.com"
        );
        assert!(normalize_host("bad host/path").is_err());
        assert!(normalize_host("").is_err());
    }

    #[test]
    fn normalize_path_prefix_trims_and_strips_git_suffix() {
        assert_eq!(
            normalize_path_prefix(Some("/group/sub/repo.git/")).expect("prefix"),
            "group/sub/repo"
        );
        assert_eq!(normalize_path_prefix(None).expect("empty"), "");
        assert!(normalize_path_prefix(Some("../bad")).is_err());
    }

    #[test]
    fn normalize_header_name_enforces_token_chars() {
        assert_eq!(
            normalize_header_name("X-GitLab-Token").expect("header"),
            "X-GitLab-Token"
        );
        assert!(normalize_header_name("bad header").is_err());
    }
}
