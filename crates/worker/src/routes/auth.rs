use worker::*;

use opensession_api::{
    db,
    oauth::{self, AuthProvidersResponse, OAuthProviderInfo},
    service, AuthRegisterRequest, AuthTokenResponse, ChangePasswordRequest, LoginRequest,
    LogoutRequest, OAuthLinkResponse, OkResponse, RefreshRequest, RegenerateKeyResponse,
    RegisterRequest, RegisterResponse, ServiceError, UserSettingsResponse, VerifyResponse,
};

use crate::crypto;
use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Create JWT + refresh token pair and store refresh token in D1.
async fn issue_tokens(env: &Env, user_id: &str, nickname: &str) -> Result<AuthTokenResponse> {
    let secret = env
        .secret("JWT_SECRET")
        .map(|s| s.to_string())
        .map_err(|_| Error::from("JWT_SECRET not configured"))?;

    let now = storage::now_unix();
    let bundle = service::prepare_token_bundle(&secret, user_id, nickname, now)
        .map_err(|e| Error::from(e.message().to_string()))?;

    let db = storage::get_d1(env)?;
    let (sql, values) = db::users::insert_refresh_token(
        &bundle.token_id,
        user_id,
        &bundle.token_hash,
        &bundle.expires_at,
    );
    db.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Ok(bundle.response)
}

/// Load a specific OAuth provider config from environment.
fn load_provider_config(env: &Env, provider_id: &str) -> Result<oauth::OAuthProviderConfig> {
    match provider_id {
        "github" => {
            let client_id = env.secret("GITHUB_CLIENT_ID")?.to_string();
            let client_secret = env.secret("GITHUB_CLIENT_SECRET")?.to_string();
            Ok(oauth::github_preset(client_id, client_secret))
        }
        "gitlab" => {
            let url = env.var("GITLAB_URL")?.to_string();
            let client_id = env.secret("GITLAB_CLIENT_ID")?.to_string();
            let client_secret = env.secret("GITLAB_CLIENT_SECRET")?.to_string();
            let ext_url = env
                .var("GITLAB_EXTERNAL_URL")
                .ok()
                .map(|v| v.to_string())
                .filter(|s| !s.is_empty());
            Ok(oauth::gitlab_preset(url, ext_url, client_id, client_secret))
        }
        _ => Err(Error::from(format!(
            "unknown OAuth provider: {provider_id}"
        ))),
    }
}

/// Load all available OAuth providers from environment.
fn load_all_providers(env: &Env) -> Vec<oauth::OAuthProviderConfig> {
    ["github", "gitlab"]
        .iter()
        .filter_map(|&id| load_provider_config(env, id).ok())
        .collect()
}

fn resolve_base_url(req: Option<&Request>, env: &Env) -> String {
    if let Some(request) = req {
        if let Ok(url) = request.url() {
            let origin = url.origin().ascii_serialization();
            if !origin.is_empty() && origin != "null" {
                return origin;
            }
        }
    }
    env.var("BASE_URL")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "https://opensession.io".to_string())
}

// ── Legacy register (nickname only, for CLI) ────────────────────────────────

/// POST /api/register
pub async fn register(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: RegisterRequest = req.json().await?;
    let nickname = match service::validate_nickname(&body.nickname) {
        Ok(n) => n,
        Err(e) => return e.into_err_response(),
    };

    let user_id = uuid::Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();
    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::users::insert(&user_id, &nickname, &api_key);
    let result = d1.prepare(&sql).bind(&values_to_js(&values))?.run().await;

    match result {
        Ok(_) => {
            let mut resp = Response::from_json(&RegisterResponse {
                user_id,
                nickname,
                api_key,
            })?;
            resp = resp.with_status(201);
            Ok(resp)
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("constraint") {
                ServiceError::Conflict("nickname already taken".into()).into_err_response()
            } else {
                ServiceError::Internal("internal server error".into()).into_err_response()
            }
        }
    }
}

// ── Email/password auth ─────────────────────────────────────────────────────

/// POST /api/auth/register
pub async fn auth_register(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: AuthRegisterRequest = req.json().await?;
    let email = match service::validate_email(&body.email) {
        Ok(e) => e,
        Err(e) => return e.into_err_response(),
    };
    if let Err(e) = service::validate_password(&body.password) {
        return e.into_err_response();
    }
    let nickname = match service::validate_nickname(&body.nickname) {
        Ok(n) => n,
        Err(e) => return e.into_err_response(),
    };

    if storage::get_user_by_email(&ctx.env, &email)
        .await?
        .is_some()
    {
        return ServiceError::Conflict("email already registered".into()).into_err_response();
    }

    let user_id = uuid::Uuid::new_v4().to_string();
    let api_key = service::generate_api_key();
    let (password_hash, password_salt) =
        crypto::hash_password(&body.password).map_err(|e| Error::from(e.message().to_string()))?;
    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::users::insert_with_email(
        &user_id,
        &nickname,
        &api_key,
        &email,
        &password_hash,
        &password_salt,
    );
    let result = d1.prepare(&sql).bind(&values_to_js(&values))?.run().await;

    match result {
        Ok(_) => {
            let tokens = issue_tokens(&ctx.env, &user_id, &nickname).await?;
            let mut resp = Response::from_json(&tokens)?;
            resp = resp.with_status(201);
            Ok(resp)
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE") || msg.contains("constraint") {
                ServiceError::Conflict("nickname already taken".into()).into_err_response()
            } else {
                ServiceError::Internal("internal server error".into()).into_err_response()
            }
        }
    }
}

/// POST /api/auth/login
pub async fn login(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: LoginRequest = req.json().await?;
    let email = match service::validate_email(&body.email) {
        Ok(e) => e,
        Err(e) => return e.into_err_response(),
    };

    let user = match storage::get_user_by_email(&ctx.env, &email).await? {
        Some(u) => u,
        None => {
            return ServiceError::Unauthorized("invalid email or password".into())
                .into_err_response()
        }
    };

    let (hash, salt) = match (&user.password_hash, &user.password_salt) {
        (Some(h), Some(s)) => (h.as_str(), s.as_str()),
        _ => {
            return ServiceError::Unauthorized(
                "this account uses OAuth login, not email/password".into(),
            )
            .into_err_response()
        }
    };

    if !crypto::verify_password(&body.password, hash, salt) {
        return ServiceError::Unauthorized("invalid email or password".into()).into_err_response();
    }

    let tokens = issue_tokens(&ctx.env, &user.id, &user.nickname).await?;
    Response::from_json(&tokens)
}

/// POST /api/auth/refresh
pub async fn refresh(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: RefreshRequest = req.json().await?;
    let token_hash = crypto::hash_token(&body.refresh_token);
    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::users::lookup_refresh_token(&token_hash);
    let row = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::RefreshTokenRow>(None)
        .await?;

    let row = match row {
        Some(r) => r,
        None => {
            return ServiceError::Unauthorized("invalid refresh token".into()).into_err_response()
        }
    };

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if row.expires_at < now {
        let (sql, values) = db::users::delete_refresh_token_by_id(&row.id);
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
        return ServiceError::Unauthorized("refresh token expired".into()).into_err_response();
    }

    let (sql, values) = db::users::delete_refresh_token(&token_hash);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    let tokens = issue_tokens(&ctx.env, &row.user_id, &row.nickname).await?;
    Response::from_json(&tokens)
}

/// POST /api/auth/logout
pub async fn logout(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: LogoutRequest = req.json().await?;
    let token_hash = crypto::hash_token(&body.refresh_token);
    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::users::delete_refresh_token(&token_hash);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Response::from_json(&OkResponse { ok: true })
}

/// PUT /api/auth/password
pub async fn change_password(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    let body: ChangePasswordRequest = req.json().await?;

    let (hash, salt) = match (&user.password_hash, &user.password_salt) {
        (Some(h), Some(s)) => (h.as_str(), s.as_str()),
        _ => {
            return ServiceError::BadRequest("cannot change password for OAuth-only account".into())
                .into_err_response()
        }
    };

    if !crypto::verify_password(&body.current_password, hash, salt) {
        return ServiceError::Unauthorized("current password is incorrect".into())
            .into_err_response();
    }
    if let Err(e) = service::validate_password(&body.new_password) {
        return e.into_err_response();
    }

    let (new_hash, new_salt) = crypto::hash_password(&body.new_password)
        .map_err(|e| Error::from(e.message().to_string()))?;
    let d1 = storage::get_d1(&ctx.env)?;
    let (sql, values) = db::users::update_password(&user.id, &new_hash, &new_salt);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Response::from_json(&OkResponse { ok: true })
}

// ── Generic OAuth ──────────────────────────────────────────────────────────

/// GET /api/auth/providers
pub async fn auth_providers(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let jwt_configured = ctx.env.secret("JWT_SECRET").is_ok();
    let providers = load_all_providers(&ctx.env);
    Response::from_json(&AuthProvidersResponse {
        email_password: jwt_configured,
        oauth: providers
            .iter()
            .map(|p| OAuthProviderInfo {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
            })
            .collect(),
    })
}

/// GET /api/auth/oauth/:provider — redirect to provider's authorize page
pub async fn oauth_redirect(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let provider_id = ctx
        .param("provider")
        .ok_or_else(|| Error::from("missing provider param"))?
        .to_string();
    oauth_redirect_inner(&req, &ctx.env, &provider_id).await
}

/// GET /api/auth/oauth/:provider/callback
pub async fn oauth_callback(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let provider_id = ctx
        .param("provider")
        .ok_or_else(|| Error::from("missing provider param"))?
        .to_string();
    oauth_callback_inner(req, &ctx.env, &provider_id).await
}

/// POST /api/auth/oauth/:provider/link
pub async fn oauth_link(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let provider_id = ctx
        .param("provider")
        .ok_or_else(|| Error::from("missing provider param"))?
        .to_string();
    oauth_link_inner(req, &ctx.env, &provider_id).await
}

// ── OAuth inner implementations ─────────────────────────────────────────────

async fn oauth_redirect_inner(req: &Request, env: &Env, provider_id: &str) -> Result<Response> {
    let provider = load_provider_config(env, provider_id)?;
    let base_url = resolve_base_url(Some(req), env);

    let state = crypto::generate_token()?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let d1 = storage::get_d1(env)?;
    let (sql, values) = db::oauth::insert_state(&state, provider_id, &expires_at, None);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    let redirect_uri = format!("{base_url}/api/auth/oauth/{}/callback", provider.id);
    let url = oauth::build_authorize_url(&provider, &redirect_uri, &state);
    Response::redirect(Url::parse(&url)?)
}

async fn oauth_callback_inner(req: Request, env: &Env, provider_id: &str) -> Result<Response> {
    let provider = load_provider_config(env, provider_id)?;
    let base_url = resolve_base_url(Some(&req), env);

    let url = req.url()?;
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let code = params
        .get("code")
        .ok_or_else(|| Error::from("missing code parameter"))?;
    let state_param = params
        .get("state")
        .ok_or_else(|| Error::from("missing state parameter"))?;

    // Validate state
    let d1 = storage::get_d1(env)?;
    let (sql, values) = db::oauth::validate_state(state_param);
    let state_row = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::OAuthStateRow>(None)
        .await?;

    let linking_user_id = match state_row {
        None => return ServiceError::BadRequest("invalid OAuth state".into()).into_err_response(),
        Some(row) => {
            if row.provider != provider_id {
                return ServiceError::BadRequest("OAuth state provider mismatch".into())
                    .into_err_response();
            }
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            if row.expires_at < now {
                return ServiceError::BadRequest("OAuth state expired".into()).into_err_response();
            }
            let (sql, values) = db::oauth::delete_state(state_param);
            d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
            row.user_id
        }
    };

    // Exchange code for access token
    let redirect_uri = format!("{base_url}/api/auth/oauth/{provider_id}/callback");
    let token_body = oauth::build_token_request_body(&provider, code, &redirect_uri);

    let mut token_resp = Fetch::Request(Request::new_with_init(
        &provider.token_url,
        RequestInit::new()
            .with_method(Method::Post)
            .with_headers({
                let h = Headers::new();
                let _ = h.set("Accept", "application/json");
                let _ = h.set("Content-Type", "application/json");
                h
            })
            .with_body(Some(token_body.to_string().into())),
    )?)
    .send()
    .await?;

    let token_json: serde_json::Value = token_resp.json().await?;
    let access_token = token_json["access_token"]
        .as_str()
        .ok_or_else(|| Error::from("OAuth token exchange failed: no access_token"))?;

    // Fetch userinfo
    let mut userinfo_resp = Fetch::Request(Request::new_with_init(
        &provider.userinfo_url,
        RequestInit::new().with_headers({
            let h = Headers::new();
            let _ = h.set("Authorization", &format!("Bearer {access_token}"));
            let _ = h.set("User-Agent", "opensession-worker");
            let _ = h.set("Accept", "application/json");
            h
        }),
    )?)
    .send()
    .await?;

    let userinfo: serde_json::Value = userinfo_resp.json().await?;

    // Fetch emails if separate endpoint configured (GitHub)
    let emails: Option<Vec<serde_json::Value>> = match provider.email_url {
        Some(ref email_url) => {
            let mut resp = Fetch::Request(Request::new_with_init(
                email_url,
                RequestInit::new().with_headers({
                    let h = Headers::new();
                    let _ = h.set("Authorization", &format!("Bearer {access_token}"));
                    let _ = h.set("User-Agent", "opensession-worker");
                    let _ = h.set("Accept", "application/json");
                    h
                }),
            )?)
            .send()
            .await?;
            resp.json().await.ok()
        }
        None => None,
    };

    let user_info = match oauth::extract_user_info(&provider, &userinfo, emails.as_deref()) {
        Ok(info) => info,
        Err(e) => return e.into_err_response(),
    };

    // ── Linking mode ──
    if let Some(ref link_uid) = linking_user_id {
        let (sql, values) = db::oauth::find_by_provider(provider_id, &user_info.provider_user_id);
        let existing: Option<storage::OAuthIdentityRow> = d1
            .prepare(&sql)
            .bind(&values_to_js(&values))?
            .first::<storage::OAuthIdentityRow>(None)
            .await?;

        if let Some(ref row) = existing {
            if row.user_id != *link_uid {
                let redirect_url = format!("{base_url}/settings?error=oauth_already_linked");
                return Response::redirect(Url::parse(&redirect_url)?);
            }
        }

        let (sql, values) = db::oauth::upsert_identity(
            link_uid,
            provider_id,
            &user_info.provider_user_id,
            Some(&user_info.username),
            user_info.avatar_url.as_deref(),
            None,
        );
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

        let redirect_url = format!("{base_url}/settings?oauth_linked=true");
        return Response::redirect(Url::parse(&redirect_url)?);
    }

    // ── Normal login/register flow ──
    let (sql, values) = db::oauth::find_by_provider(provider_id, &user_info.provider_user_id);
    let existing: Option<storage::OAuthIdentityRow> = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::OAuthIdentityRow>(None)
        .await?;

    let (user_id, nickname) = if let Some(row) = existing {
        // Update provider info
        let (sql, values) = db::oauth::upsert_identity(
            &row.user_id,
            provider_id,
            &user_info.provider_user_id,
            Some(&user_info.username),
            user_info.avatar_url.as_deref(),
            None,
        );
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

        let (sql, values) = db::users::get_nickname(&row.user_id);
        let nick = d1
            .prepare(&sql)
            .bind(&values_to_js(&values))?
            .first::<storage::NicknameRow>(None)
            .await?
            .map(|r| r.nickname)
            .unwrap_or_else(|| user_info.username.clone());

        (row.user_id, nick)
    } else {
        // Check if email matches existing user (auto-link)
        let by_email = if let Some(ref email) = user_info.email {
            storage::get_user_by_email(env, email).await?
        } else {
            None
        };

        if let Some(user) = by_email {
            let (sql, values) = db::oauth::upsert_identity(
                &user.id,
                provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
                None,
            );
            d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
            (user.id, user.nickname)
        } else {
            // Create new user
            let new_id = uuid::Uuid::new_v4().to_string();
            let api_key = service::generate_api_key();

            let (sql, values) = db::users::insert_oauth(
                &new_id,
                &user_info.username,
                &api_key,
                user_info.email.as_deref(),
            );
            d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

            let (sql, values) = db::oauth::upsert_identity(
                &new_id,
                provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
                None,
            );
            d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

            (new_id, user_info.username)
        }
    };

    let tokens = issue_tokens(env, &user_id, &nickname).await?;

    let redirect_url = format!(
        "{base_url}/auth/callback#access_token={}&refresh_token={}&expires_in={}",
        tokens.access_token, tokens.refresh_token, tokens.expires_in,
    );
    Response::redirect(Url::parse(&redirect_url)?)
}

async fn oauth_link_inner(req: Request, env: &Env, provider_id: &str) -> Result<Response> {
    let provider = load_provider_config(env, provider_id)?;
    let base_url = resolve_base_url(Some(&req), env);

    let user = match storage::require_auth(&req, env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let d1 = storage::get_d1(env)?;
    let (sql, values) = db::oauth::has_provider(&user.id, provider_id);
    let already = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if already > 0 {
        return ServiceError::Conflict(format!("{} account already linked", provider.display_name))
            .into_err_response();
    }

    let state = crypto::generate_token()?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let (sql, values) = db::oauth::insert_state(&state, provider_id, &expires_at, Some(&user.id));
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    let redirect_uri = format!("{base_url}/api/auth/oauth/{provider_id}/callback");
    let url = oauth::build_authorize_url(&provider, &redirect_uri, &state);
    Response::from_json(&OAuthLinkResponse { url })
}

// ── Existing endpoints ──────────────────────────────────────────────────────

/// POST /api/auth/verify
pub async fn verify(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };
    Response::from_json(&VerifyResponse {
        user_id: user.id,
        nickname: user.nickname,
    })
}

/// GET /api/auth/me
pub async fn me(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::auth_from_req(&req, &ctx.env).await {
        Ok(u) => u,
        Err(_) => return ServiceError::Unauthorized("unauthorized".into()).into_err_response(),
    };

    // Fetch linked OAuth providers
    let d1 = storage::get_d1(&ctx.env)?;
    let (sql, values) = db::oauth::find_by_user(&user.id);
    let oauth_rows = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .all()
        .await?
        .results::<storage::ProviderRow>()?;

    let all_providers = load_all_providers(&ctx.env);
    let linked_providers: Vec<oauth::LinkedProvider> = oauth_rows
        .iter()
        .map(|row| {
            let display_name = all_providers
                .iter()
                .find(|p| p.id == row.provider)
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| row.provider.clone());
            oauth::LinkedProvider {
                provider: row.provider.clone(),
                provider_username: row.provider_username.clone().unwrap_or_default(),
                display_name,
            }
        })
        .collect();

    // Legacy: GitHub username from linked providers
    let github_username = linked_providers
        .iter()
        .find(|p| p.provider == "github")
        .map(|p| p.provider_username.clone());

    // Avatar: prefer linked provider, fallback to user row
    let avatar_url = oauth_rows
        .iter()
        .find_map(|r| r.avatar_url.clone())
        .or(user.avatar_url);

    Response::from_json(&UserSettingsResponse {
        user_id: user.id,
        nickname: user.nickname,
        api_key: user.api_key,
        created_at: user.created_at,
        email: user.email,
        avatar_url,
        oauth_providers: linked_providers,
        github_username,
    })
}

/// POST /api/auth/regenerate-key
pub async fn regenerate_key(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let new_key = service::generate_api_key();
    let d1 = storage::get_d1(&ctx.env)?;
    let (sql, values) = db::users::update_api_key(&user.id, &new_key);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Response::from_json(&RegenerateKeyResponse { api_key: new_key })
}
