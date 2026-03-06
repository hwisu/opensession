use opensession_api::{AuthTokenResponse, ServiceError, crypto};
use serde::{Deserialize, Serialize};
use worker::*;

use crate::config::WorkerConfig;
use crate::db_helpers::values_to_js;

pub(super) type ServiceResult<T> = std::result::Result<T, ServiceError>;

pub(super) const ACCESS_COOKIE_NAME: &str = "opensession_access_token";
pub(super) const REFRESH_COOKIE_NAME: &str = "opensession_refresh_token";
pub(super) const CSRF_COOKIE_NAME: &str = "opensession_csrf_token";
pub(super) const CSRF_HEADER_NAME: &str = "x-csrf-token";

#[derive(Debug, Deserialize)]
pub(super) struct UserRow {
    pub(super) id: String,
    pub(super) nickname: String,
    pub(super) email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LoginRow {
    pub(super) id: String,
    pub(super) nickname: String,
    pub(super) password_hash: Option<String>,
    pub(super) password_salt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RefreshRow {
    pub(super) id: String,
    pub(super) user_id: String,
    pub(super) expires_at: String,
    pub(super) nickname: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SettingsRow {
    pub(super) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OAuthIdentityRow {
    pub(super) provider: String,
    pub(super) provider_username: Option<String>,
    pub(super) avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OAuthStateRow {
    pub(super) provider: String,
    pub(super) expires_at: String,
    pub(super) user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OAuthIdentityUserRow {
    pub(super) user_id: String,
}

pub(super) fn now_unix() -> u64 {
    let now = chrono::Utc::now().timestamp();
    if now < 0 { 0 } else { now as u64 }
}

pub(super) fn now_sqlite_datetime() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn parse_cookie_value(headers: &Headers, name: &str) -> Option<String> {
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

pub(super) fn auth_cookie_values(
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

pub(super) fn clear_auth_cookie_values(req: &Request, config: &WorkerConfig) -> Vec<String> {
    let secure = secure_cookie_mode(req, config);
    vec![
        build_set_cookie(ACCESS_COOKIE_NAME, "", 0, "/api", true, secure),
        build_set_cookie(REFRESH_COOKIE_NAME, "", 0, "/api", true, secure),
        build_set_cookie(CSRF_COOKIE_NAME, "", 0, "/", false, secure),
    ]
}

pub(super) fn enforce_csrf_if_cookie_auth(
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
        .ok_or_else(|| ServiceError::Unauthorized("missing request origin".into()))?;
    if !config.allowed_origins.iter().any(|value| value == &origin) {
        return Err(ServiceError::Unauthorized(
            "request origin is not allowed".into(),
        ));
    }
    let csrf_cookie = parse_cookie_value(req.headers(), CSRF_COOKIE_NAME)
        .ok_or_else(|| ServiceError::Unauthorized("missing csrf cookie".into()))?;
    let csrf_header = req
        .headers()
        .get(CSRF_HEADER_NAME)
        .map_err(|e| service_internal("read csrf header", e))?
        .ok_or_else(|| ServiceError::Unauthorized("missing csrf header".into()))?;
    if !constant_time_eq(&csrf_cookie, &csrf_header) {
        return Err(ServiceError::Unauthorized("csrf token mismatch".into()));
    }
    Ok(())
}

pub(super) fn service_internal(
    context: &str,
    err: impl std::fmt::Display,
) -> ServiceError {
    ServiceError::Internal(format!("{context}: {err}"))
}

pub(super) fn json_response<T: Serialize>(value: &T, status: u16) -> Result<Response> {
    Response::from_json(value).map(|resp| resp.with_status(status))
}

pub(super) fn json_response_with_cookies<T: Serialize>(
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

pub(super) async fn parse_json<T: for<'de> Deserialize<'de>>(
    req: &mut Request,
) -> ServiceResult<T> {
    req.json()
        .await
        .map_err(|_| ServiceError::BadRequest("invalid request body".into()))
}

pub(super) async fn d1_first<T: for<'de> Deserialize<'de>>(
    d1: &D1Database,
    built: (String, sea_query::Values),
    context: &str,
) -> ServiceResult<Option<T>> {
    let (sql, values) = built;
    let stmt = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))
        .map_err(|e| service_internal(context, e))?;
    let result = stmt.first(None).await;
    result.map_err(|e| service_internal(context, e))
}

pub(super) async fn d1_all<T: for<'de> Deserialize<'de>>(
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

pub(super) async fn d1_run(
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
        return Err(ServiceError::Internal(
            result.error().unwrap_or_else(|| format!("{context} failed")),
        ));
    }
    Ok(())
}
