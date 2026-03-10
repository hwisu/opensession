use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Redirect, Response},
};
use uuid::Uuid;

use opensession_api::{
    OAuthLinkResponse, crypto,
    oauth::{self, AuthProvidersResponse, OAuthProviderConfig, OAuthProviderInfo},
};

use super::auth::AuthUser;
use crate::error::ApiErr;
use crate::storage::Db;
use crate::{AppConfig, AppState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_provider<'a>(config: &'a AppConfig, id: &str) -> Result<&'a OAuthProviderConfig, ApiErr> {
    config
        .oauth_providers
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| ApiErr::not_found(format!("OAuth provider '{}' not found", id)))
}

fn first_header_value(headers: &HeaderMap, key: header::HeaderName) -> Option<String> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_base_url(headers: &HeaderMap, fallback: &str, prefer_request_host: bool) -> String {
    let fallback = fallback.trim_end_matches('/').to_string();
    if !prefer_request_host {
        return fallback;
    }

    let host = first_header_value(headers, header::HeaderName::from_static("x-forwarded-host"))
        .or_else(|| first_header_value(headers, header::HOST));
    let proto = first_header_value(
        headers,
        header::HeaderName::from_static("x-forwarded-proto"),
    );

    match host {
        Some(host) => {
            let scheme = proto.unwrap_or_else(|| {
                if fallback.starts_with("http://") {
                    "http".to_string()
                } else {
                    "https".to_string()
                }
            });
            format!("{scheme}://{host}")
        }
        None => fallback,
    }
}

async fn maybe_store_provider_access_token(
    db: &Db,
    config: &AppConfig,
    user_id: &str,
    provider: &OAuthProviderConfig,
    access_token: &str,
) -> Result<(), ApiErr> {
    let Some(keyring) = config.credential_keyring.as_ref() else {
        return Ok(());
    };
    let provider_host = oauth_provider_host(provider)?;
    let encrypted = keyring.encrypt(access_token).map_err(ApiErr::from)?;
    let token_id = Uuid::new_v4().to_string();
    db.upsert_oauth_provider_access_token(
        &token_id,
        user_id,
        &provider.id,
        &provider_host,
        &encrypted,
    )
    .await
    .map_err(ApiErr::from_db("oauth provider token upsert"))?;
    Ok(())
}

fn oauth_provider_host(provider: &OAuthProviderConfig) -> Result<String, ApiErr> {
    let parsed = reqwest::Url::parse(&provider.token_url)
        .map_err(|_| ApiErr::internal("invalid OAuth provider token URL"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ApiErr::internal("OAuth provider token URL missing host"))?;
    Ok(host.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// GET /api/auth/providers — list available auth methods
// ---------------------------------------------------------------------------

/// GET /api/auth/providers — list available authentication methods.
pub async fn providers(State(config): State<AppConfig>) -> Json<AuthProvidersResponse> {
    Json(AuthProvidersResponse {
        email_password: !config.jwt_secret.is_empty(),
        oauth: config
            .oauth_providers
            .iter()
            .map(|p| OAuthProviderInfo {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// GET /api/auth/oauth/:provider — redirect to provider's authorize page
// ---------------------------------------------------------------------------

/// GET /api/auth/oauth/:provider — redirect to provider's authorize page.
pub async fn redirect(
    Path(provider_id): Path<String>,
    State(db): State<Db>,
    State(config): State<AppConfig>,
    headers: HeaderMap,
) -> Result<Redirect, ApiErr> {
    let provider = find_provider(&config, &provider_id)?;

    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    let state = crypto::generate_token().map_err(ApiErr::from)?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.insert_oauth_state(&state, &provider_id, &expires_at, None)
        .await
        .map_err(ApiErr::from_db("oauth state insert"))?;

    let base_url = resolve_base_url(&headers, &config.base_url, config.oauth_use_request_host);
    let redirect_uri = format!("{}/api/auth/oauth/{}/callback", base_url, provider_id);
    let url = oauth::build_authorize_url(provider, &redirect_uri, &state);

    Ok(Redirect::temporary(&url))
}

// ---------------------------------------------------------------------------
// GET /api/auth/oauth/:provider/callback — handle OAuth callback
// ---------------------------------------------------------------------------

/// GET /api/auth/oauth/:provider/callback — handle OAuth callback (login or register).
pub async fn callback(
    Path(provider_id): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiErr> {
    let db = state.db.clone();
    let config = state.config.clone();
    let provider = find_provider(&config, &provider_id)?;
    let base_url = resolve_base_url(&headers, &config.base_url, config.oauth_use_request_host);

    let code = params
        .get("code")
        .ok_or_else(|| ApiErr::bad_request("missing code parameter"))?;
    let state_param = params
        .get("state")
        .ok_or_else(|| ApiErr::bad_request("missing state parameter"))?;

    // Validate state (scope the MutexGuard so it's dropped before await)
    let state_row = db
        .validate_oauth_state(state_param)
        .await
        .map_err(|_| ApiErr::bad_request("invalid OAuth state"))?;
    if state_row.provider != provider_id {
        return Err(ApiErr::bad_request("OAuth state provider mismatch"));
    }
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if state_row.expires_at < now {
        return Err(ApiErr::bad_request("OAuth state expired"));
    }
    db.delete_oauth_state(state_param).await.ok();
    let linking_user_id = state_row.user_id;

    // Exchange code for access token
    let redirect_uri = format!("{}/api/auth/oauth/{}/callback", base_url, provider_id);
    let token_form = oauth::build_token_request_form(provider, code, &redirect_uri);

    let client = reqwest::Client::new();
    let token_resp = client
        .post(&provider.token_url)
        .header("Accept", "application/json")
        .form(&token_form)
        .send()
        .await
        .map_err(|e| ApiErr::internal(format!("token exchange failed: {e}")))?;
    let token_status = token_resp.status();
    let token_raw = token_resp
        .text()
        .await
        .map_err(|e| ApiErr::internal(format!("token response read failed: {e}")))?;

    let access_token = oauth::parse_access_token_response(&token_raw).map_err(|e| {
        let mut msg = if token_status.is_success() {
            e.message().to_string()
        } else {
            format!("{} (status {token_status})", e.message())
        };
        if msg.contains("incorrect_client_credentials") {
            msg.push_str(match provider_id.as_str() {
                "github" => {
                    "; verify GITHUB_CLIENT_ID/GITHUB_CLIENT_SECRET match the GitHub OAuth app"
                }
                "gitlab" => {
                    "; verify GITLAB_CLIENT_ID/GITLAB_CLIENT_SECRET match the GitLab OAuth app"
                }
                _ => "; verify OAuth client credentials",
            });
        }
        ApiErr::internal(msg)
    })?;

    // Fetch userinfo
    let userinfo: serde_json::Value = client
        .get(&provider.userinfo_url)
        .bearer_auth(&access_token)
        .header("User-Agent", "opensession-server")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ApiErr::internal(format!("userinfo fetch failed: {e}")))?
        .json()
        .await
        .map_err(|e| ApiErr::internal(format!("userinfo parse failed: {e}")))?;

    // Fetch emails if separate endpoint configured (GitHub)
    let emails: Option<Vec<serde_json::Value>> = match provider.email_url {
        Some(ref email_url) => match client
            .get(email_url)
            .bearer_auth(&access_token)
            .header("User-Agent", "opensession-server")
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(r) => r.json().await.ok(),
            Err(_) => None,
        },
        None => None,
    };

    let user_info =
        oauth::extract_user_info(provider, &userinfo, emails.as_deref()).map_err(ApiErr::from)?;

    // ── Linking mode ──
    if let Some(ref link_uid) = linking_user_id {
        // Check if this provider identity is already linked to another account
        let existing_user = db
            .find_oauth_user_id_by_provider(&provider_id, &user_info.provider_user_id)
            .await
            .map_err(ApiErr::from_db("oauth link lookup"))?;

        if let Some(ref existing) = existing_user {
            if existing != link_uid {
                return Ok(Redirect::temporary(&format!(
                    "{}/settings?error=oauth_already_linked",
                    base_url
                ))
                .into_response());
            }
        }

        db.upsert_oauth_identity(
            link_uid,
            &provider_id,
            &user_info.provider_user_id,
            Some(&user_info.username),
            user_info.avatar_url.as_deref(),
        )
        .await
        .map_err(ApiErr::from_db("oauth link upsert"))?;
        maybe_store_provider_access_token(&db, &config, link_uid, provider, &access_token).await?;

        return Ok(
            Redirect::temporary(&format!("{}/settings?oauth_linked=true", base_url))
                .into_response(),
        );
    }

    // ── Normal login/register flow ──

    // Check if OAuth identity already exists
    let existing_user_id = db
        .find_oauth_user_id_by_provider(&provider_id, &user_info.provider_user_id)
        .await
        .map_err(ApiErr::from_db("oauth identity lookup"))?;

    let (user_id, nickname) = if let Some(uid) = existing_user_id {
        // Update provider info
        db.upsert_oauth_identity(
            &uid,
            &provider_id,
            &user_info.provider_user_id,
            Some(&user_info.username),
            user_info.avatar_url.as_deref(),
        )
        .await
        .ok();

        let nick = db
            .get_user_nickname(&uid)
            .await
            .unwrap_or_else(|_| user_info.username.clone());

        (uid, nick)
    } else {
        // Check if email matches existing user (auto-link)
        let by_email = match user_info.email.as_deref() {
            Some(email) => db.get_user_id_and_nickname_by_email(email).await.ok(),
            None => None,
        };

        if let Some((uid, nick)) = by_email {
            db.upsert_oauth_identity(
                &uid,
                &provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
            )
            .await
            .ok();
            (uid, nick)
        } else {
            // Create new user
            let user_id = Uuid::new_v4().to_string();
            let username = user_info.username.clone();

            // OAuth users have no password — insert with email but empty hash/salt
            db.insert_oauth_user(&user_id, &username, user_info.email.as_deref())
                .await
                .map_err(ApiErr::from_db("create user from oauth"))?;

            db.upsert_oauth_identity(
                &user_id,
                &provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
            )
            .await
            .map_err(ApiErr::from_db("oauth identity insert"))?;

            (user_id, username)
        }
    };
    maybe_store_provider_access_token(&db, &config, &user_id, provider, &access_token).await?;

    // Issue tokens
    let tokens =
        super::auth::issue_tokens_pub(&db, &config.jwt_secret, &user_id, &nickname).await?;

    // Redirect to frontend without exposing tokens in URL fragments.
    let redirect_url = format!("{}/auth/callback", base_url);
    let mut response = Redirect::temporary(&redirect_url).into_response();
    let cookies = super::auth::set_cookie_headers_for_auth(&tokens, &headers, &config)?;
    for cookie in cookies {
        let value = HeaderValue::from_str(&cookie)
            .map_err(|_| ApiErr::internal("failed to set auth cookie"))?;
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}

// ---------------------------------------------------------------------------
// POST /api/auth/oauth/:provider/link — initiate linking for authenticated user
// ---------------------------------------------------------------------------

/// POST /api/auth/oauth/:provider/link — initiate OAuth linking for authenticated user.
pub async fn link(
    Path(provider_id): Path<String>,
    State(db): State<Db>,
    State(config): State<AppConfig>,
    headers: HeaderMap,
    user: AuthUser,
) -> Result<Json<OAuthLinkResponse>, ApiErr> {
    super::auth::enforce_csrf_if_cookie_auth(&headers, &config, user.auth_via_cookie)?;
    let provider = find_provider(&config, &provider_id)?;

    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    // Check if already linked
    let already = db
        .user_has_oauth_provider(&user.user_id, &provider_id)
        .await
        .unwrap_or(false);
    if already {
        return Err(ApiErr::conflict(format!(
            "{} account already linked",
            provider.display_name
        )));
    }

    let state = crypto::generate_token().map_err(ApiErr::from)?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.insert_oauth_state(&state, &provider_id, &expires_at, Some(&user.user_id))
        .await
        .map_err(ApiErr::from_db("oauth state insert for link"))?;

    let base_url = resolve_base_url(&headers, &config.base_url, config.oauth_use_request_host);
    let redirect_uri = format!("{}/api/auth/oauth/{}/callback", base_url, provider_id);
    let url = oauth::build_authorize_url(provider, &redirect_uri, &state);

    Ok(Json(OAuthLinkResponse { url }))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::resolve_base_url;

    #[test]
    fn resolve_base_url_prefers_config_when_request_host_disabled() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("localhost:5173"));
        headers.insert(
            header::HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static("https"),
        );

        let resolved = resolve_base_url(&headers, "https://app.example.com/", false);
        assert_eq!(resolved, "https://app.example.com");
    }

    #[test]
    fn resolve_base_url_uses_forwarded_host_when_enabled() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HeaderName::from_static("x-forwarded-host"),
            HeaderValue::from_static("edge.example.com"),
        );
        headers.insert(
            header::HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static("https"),
        );

        let resolved = resolve_base_url(&headers, "http://localhost:3000", true);
        assert_eq!(resolved, "https://edge.example.com");
    }

    #[test]
    fn resolve_base_url_falls_back_without_host_header() {
        let headers = HeaderMap::new();
        let resolved = resolve_base_url(&headers, "http://localhost:3000/", true);
        assert_eq!(resolved, "http://localhost:3000");
    }
}
