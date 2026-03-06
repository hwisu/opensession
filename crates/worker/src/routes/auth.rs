mod git_credentials;
mod oauth_flow;
mod support;

pub use git_credentials::{create_git_credential, delete_git_credential, list_git_credentials};
pub use oauth_flow::{oauth_callback, oauth_redirect};

use opensession_api::{
    AuthRegisterRequest, AuthTokenResponse, IssueApiKeyResponse, LoginRequest, LogoutRequest,
    OkResponse, RefreshRequest, UserSettingsResponse, VerifyResponse, crypto, db as dbq, oauth,
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
    service,
    service::AuthToken,
};
use uuid::Uuid;
use worker::{D1Database, Request, Response, Result, RouteContext};

use crate::config::WorkerConfig;
use crate::error::IntoErrResponse;
use crate::storage;

use self::oauth_flow::provider_display_name;
use self::support::{
    ACCESS_COOKIE_NAME, LoginRow, OAuthIdentityRow, RefreshRow, ServiceResult, SettingsRow,
    UserRow, auth_cookie_values, clear_auth_cookie_values, d1_all, d1_first, d1_run,
    enforce_csrf_if_cookie_auth, json_response, json_response_with_cookies, now_sqlite_datetime,
    now_unix, parse_cookie_value, parse_json, service_internal,
};

#[derive(Debug)]
pub(crate) struct AuthUser {
    pub(crate) user_id: String,
    pub(crate) nickname: String,
    pub(crate) auth_via_cookie: bool,
    pub(crate) email: Option<String>,
}

fn load_config_and_d1(ctx: &RouteContext<()>) -> ServiceResult<(WorkerConfig, D1Database)> {
    let config = WorkerConfig::from_env(&ctx.env);
    let d1 = storage::get_d1(&ctx.env)
        .map_err(|err| service_internal("load d1 binding", err))?;
    Ok((config, d1))
}

pub(crate) async fn authenticate(
    req: &Request,
    d1: &D1Database,
    config: &WorkerConfig,
) -> ServiceResult<AuthUser> {
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
) -> ServiceResult<Option<AuthUser>> {
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
                ));
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
        let cookie_token = parse_cookie_value(req.headers(), support::REFRESH_COOKIE_NAME);
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
        let cookie_token = parse_cookie_value(req.headers(), support::REFRESH_COOKIE_NAME);
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
    let (config, d1) = match load_config_and_d1(&ctx) {
        Ok(values) => values,
        Err(err) => return err.into_err_response(),
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
