use opensession_api::{
    ServiceError, db as dbq,
    oauth::{self, OAuthProviderConfig},
};
use uuid::Uuid;
use worker::*;

use crate::config::WorkerConfig;
use crate::error::IntoErrResponse;

use super::issue_tokens;
use super::support::{
    LoginRow, OAuthIdentityUserRow, OAuthStateRow, ServiceResult, UserRow, auth_cookie_values,
    d1_first, d1_run, now_sqlite_datetime, service_internal,
};

fn find_provider<'a>(
    config: &'a WorkerConfig,
    provider_id: &str,
) -> ServiceResult<&'a OAuthProviderConfig> {
    config
        .oauth_providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| ServiceError::NotFound(format!("OAuth provider '{provider_id}' not found")))
}

pub(super) fn provider_display_name(provider_id: &str) -> String {
    match provider_id {
        "github" => "GitHub".to_string(),
        "gitlab" => "GitLab".to_string(),
        other => other.to_string(),
    }
}

fn oauth_provider_host(provider: &OAuthProviderConfig) -> ServiceResult<String> {
    let parsed = Url::parse(&provider.token_url)
        .map_err(|_| ServiceError::Internal("invalid OAuth provider token URL".into()))?;
    let host = parsed.host_str().ok_or_else(|| {
        ServiceError::Internal("OAuth provider token URL missing host".into())
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

pub async fn oauth_redirect(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let (config, d1) = match super::load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
    };

    let result: ServiceResult<String> = async {
        if config.jwt_secret.is_empty() {
            return Err(ServiceError::Internal("JWT_SECRET not configured".into()));
        }

        let provider_id = ctx
            .param("provider")
            .ok_or_else(|| ServiceError::BadRequest("missing provider".into()))?;
        let provider = find_provider(&config, provider_id)?;

        let state = opensession_api::crypto::generate_token()?;
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
    let (config, d1) = match super::load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
    };

    let result: ServiceResult<(String, Vec<String>)> = async {
        let provider_id = ctx
            .param("provider")
            .ok_or_else(|| ServiceError::BadRequest("missing provider".into()))?
            .to_string();
        let provider = find_provider(&config, &provider_id)?;
        let base_url = resolve_base_url(&req, &config);

        let query: std::collections::HashMap<String, String> = req
            .url()
            .map_err(|e| service_internal("parse callback url", e))?
            .query_pairs()
            .into_owned()
            .collect();
        let code = query
            .get("code")
            .cloned()
            .ok_or_else(|| ServiceError::BadRequest("missing code parameter".into()))?;
        let state_param = query
            .get("state")
            .cloned()
            .ok_or_else(|| ServiceError::BadRequest("missing state parameter".into()))?;

        let state_row: Option<OAuthStateRow> = d1_first(
            &d1,
            dbq::oauth::validate_state(&state_param),
            "validate oauth state",
        )
        .await?;
        let state_row =
            state_row.ok_or_else(|| ServiceError::BadRequest("invalid OAuth state".into()))?;

        if state_row.provider != provider_id {
            return Err(ServiceError::BadRequest("OAuth state provider mismatch".into()));
        }
        if state_row.expires_at < now_sqlite_datetime() {
            return Err(ServiceError::BadRequest("OAuth state expired".into()));
        }
        if state_row.user_id.is_some() {
            return Err(ServiceError::BadRequest(
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
            ServiceError::Internal(msg)
        })?;

        let userinfo = fetch_json(
            &provider.userinfo_url,
            &access_token,
            "fetch oauth userinfo",
        )
        .await?;
        let emails = match provider.email_url.as_ref() {
            Some(email_url) => fetch_json(email_url, &access_token, "fetch oauth emails")
                .await
                .ok()
                .and_then(|json| json.as_array().map(|arr| arr.to_vec())),
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
            let existing_user = existing_user
                .ok_or_else(|| ServiceError::Unauthorized("user not found".into()))?;

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

pub(super) async fn fetch_text(
    req: Request,
    context: &str,
) -> ServiceResult<(u16, String)> {
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

pub(super) async fn fetch_json(
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
    let response = Fetch::Request(req).send().await;
    let mut resp = response.map_err(|e| service_internal(context, e))?;
    let json = resp.json::<serde_json::Value>().await;
    json.map_err(|e| service_internal(context, e))
}

pub(super) fn redirect_response(location: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Location", location)?;
    Ok(Response::empty()?.with_status(302).with_headers(headers))
}

pub(super) fn redirect_response_with_cookies(
    location: &str,
    cookies: &[String],
) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Location", location)?;
    for cookie in cookies {
        headers.append("Set-Cookie", cookie)?;
    }
    Ok(Response::empty()?.with_status(302).with_headers(headers))
}
