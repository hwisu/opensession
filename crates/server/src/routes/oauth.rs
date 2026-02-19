use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap},
    response::Redirect,
    Json,
};
use uuid::Uuid;

use opensession_api::{
    crypto, db as dbq,
    oauth::{self, AuthProvidersResponse, OAuthProviderConfig, OAuthProviderInfo},
    service, OAuthLinkResponse,
};

use super::auth::AuthUser;
use crate::error::ApiErr;
use crate::storage::{sq_execute, sq_query_row, Db};
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

    let conn = db.conn();
    sq_execute(
        &conn,
        dbq::oauth::insert_state(&state, &provider_id, &expires_at, None),
    )
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
) -> Result<Redirect, ApiErr> {
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
    let (_state_provider, linking_user_id) = {
        let conn = db.conn();
        let state_row = sq_query_row(&conn, dbq::oauth::validate_state(state_param), |row| {
            Ok((
                row.get::<_, String>(1)?,         // provider
                row.get::<_, String>(2)?,         // expires_at
                row.get::<_, Option<String>>(3)?, // user_id
            ))
        })
        .map_err(|_| ApiErr::bad_request("invalid OAuth state"))?;

        let (sp, expires_at, lu) = state_row;

        if sp != provider_id {
            return Err(ApiErr::bad_request("OAuth state provider mismatch"));
        }

        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if expires_at < now {
            return Err(ApiErr::bad_request("OAuth state expired"));
        }

        // Delete used state
        sq_execute(&conn, dbq::oauth::delete_state(state_param)).ok();
        (sp, lu)
    }; // conn dropped here, before any .await

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

    let conn = db.conn();

    // ── Linking mode ──
    if let Some(ref link_uid) = linking_user_id {
        // Check if this provider identity is already linked to another account
        let existing_user: Option<String> = sq_query_row(
            &conn,
            dbq::oauth::find_by_provider(&provider_id, &user_info.provider_user_id),
            |row| row.get(0),
        )
        .ok();

        if let Some(ref existing) = existing_user {
            if existing != link_uid {
                return Ok(Redirect::temporary(&format!(
                    "{}/settings?error=oauth_already_linked",
                    base_url
                )));
            }
        }

        sq_execute(
            &conn,
            dbq::oauth::upsert_identity(
                link_uid,
                &provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
                None,
            ),
        )
        .map_err(ApiErr::from_db("oauth link upsert"))?;

        return Ok(Redirect::temporary(&format!(
            "{}/settings?oauth_linked=true",
            base_url
        )));
    }

    // ── Normal login/register flow ──

    // Check if OAuth identity already exists
    let existing_user_id: Option<String> = sq_query_row(
        &conn,
        dbq::oauth::find_by_provider(&provider_id, &user_info.provider_user_id),
        |row| row.get(0),
    )
    .ok();

    let (user_id, nickname) = if let Some(uid) = existing_user_id {
        // Update provider info
        sq_execute(
            &conn,
            dbq::oauth::upsert_identity(
                &uid,
                &provider_id,
                &user_info.provider_user_id,
                Some(&user_info.username),
                user_info.avatar_url.as_deref(),
                None,
            ),
        )
        .ok();

        let nick: String = sq_query_row(
            &conn,
            dbq::users::get_by_id(&uid),
            |row| row.get(1), // col 1 = nickname
        )
        .unwrap_or_else(|_| user_info.username.clone());

        (uid, nick)
    } else {
        // Check if email matches existing user (auto-link)
        let by_email = user_info.email.as_ref().and_then(|email| {
            sq_query_row(&conn, dbq::users::get_by_email_for_login(email), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .ok()
        });

        if let Some((uid, nick)) = by_email {
            sq_execute(
                &conn,
                dbq::oauth::upsert_identity(
                    &uid,
                    &provider_id,
                    &user_info.provider_user_id,
                    Some(&user_info.username),
                    user_info.avatar_url.as_deref(),
                    None,
                ),
            )
            .ok();
            (uid, nick)
        } else {
            // Create new user
            let user_id = Uuid::new_v4().to_string();
            let username = user_info.username.clone();
            let api_key_placeholder = service::generate_api_key_placeholder(&user_id);

            // OAuth users have no password — insert with email but empty hash/salt
            sq_execute(
                &conn,
                dbq::users::insert_oauth(
                    &user_id,
                    &username,
                    &api_key_placeholder,
                    user_info.email.as_deref(),
                ),
            )
            .map_err(ApiErr::from_db("create user from oauth"))?;

            sq_execute(
                &conn,
                dbq::oauth::upsert_identity(
                    &user_id,
                    &provider_id,
                    &user_info.provider_user_id,
                    Some(&user_info.username),
                    user_info.avatar_url.as_deref(),
                    None,
                ),
            )
            .map_err(ApiErr::from_db("oauth identity insert"))?;

            (user_id, username)
        }
    };
    drop(conn);

    // Issue tokens
    let tokens = super::auth::issue_tokens_pub(&db, &config.jwt_secret, &user_id, &nickname)?;

    // Redirect to frontend with tokens in URL fragment
    let redirect_url = format!(
        "{}/auth/callback#access_token={}&refresh_token={}&expires_in={}",
        base_url, tokens.access_token, tokens.refresh_token, tokens.expires_in,
    );

    Ok(Redirect::temporary(&redirect_url))
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
    let provider = find_provider(&config, &provider_id)?;

    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    // Check if already linked
    let conn = db.conn();
    let count: i64 = sq_query_row(
        &conn,
        dbq::oauth::has_provider(&user.user_id, &provider_id),
        |row| row.get(0),
    )
    .unwrap_or(0);
    let already = count > 0;
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

    sq_execute(
        &conn,
        dbq::oauth::insert_state(&state, &provider_id, &expires_at, Some(&user.user_id)),
    )
    .map_err(ApiErr::from_db("oauth state insert for link"))?;

    let base_url = resolve_base_url(&headers, &config.base_url, config.oauth_use_request_host);
    let redirect_uri = format!("{}/api/auth/oauth/{}/callback", base_url, provider_id);
    let url = oauth::build_authorize_url(provider, &redirect_uri, &state);

    Ok(Json(OAuthLinkResponse { url }))
}

#[cfg(test)]
mod tests {
    use axum::http::{header, HeaderMap, HeaderValue};

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
