use axum::{
    extract::{Path, Query, State},
    response::Redirect,
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
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

fn is_first_user(db: &Db) -> bool {
    let conn = db.conn();
    sq_query_row(&conn, dbq::users::count(), |row| row.get::<_, i64>(0)).unwrap_or(0) == 0
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
) -> Result<Redirect, ApiErr> {
    let provider = find_provider(&config, &provider_id)?;

    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    let state = crypto::generate_token();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let conn = db.conn();
    sq_execute(
        &conn,
        dbq::oauth::insert_state(&state, &provider_id, &expires_at, None),
    )
    .map_err(ApiErr::from_db("oauth state insert"))?;

    let redirect_uri = format!(
        "{}/api/auth/oauth/{}/callback",
        config.base_url, provider_id
    );
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
) -> Result<Redirect, ApiErr> {
    let db = state.db.clone();
    let config = state.config.clone();
    let provider = find_provider(&config, &provider_id)?;

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
    let redirect_uri = format!(
        "{}/api/auth/oauth/{}/callback",
        config.base_url, provider_id
    );
    let token_body = oauth::build_token_request_body(provider, code, &redirect_uri);

    let client = reqwest::Client::new();
    let token_resp = client
        .post(&provider.token_url)
        .header("Accept", "application/json")
        .json(&token_body)
        .send()
        .await
        .map_err(|e| ApiErr::internal(format!("token exchange failed: {e}")))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| ApiErr::internal(format!("token response parse failed: {e}")))?;

    let access_token = token_resp["access_token"]
        .as_str()
        .ok_or_else(|| ApiErr::internal("OAuth token exchange failed: no access_token"))?;

    // Fetch userinfo
    let userinfo: serde_json::Value = client
        .get(&provider.userinfo_url)
        .bearer_auth(access_token)
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
            .bearer_auth(access_token)
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
                    config.base_url
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
            config.base_url
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
            let api_key = service::generate_api_key();
            let admin = is_first_user(&db);

            // OAuth users have no password — insert with email but empty hash/salt
            conn.execute(
                "INSERT INTO users (id, nickname, api_key, is_admin, email) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![user_id, username, api_key, admin, user_info.email],
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
        config.base_url, tokens.access_token, tokens.refresh_token, tokens.expires_in,
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
    user: AuthUser,
) -> Result<Json<OAuthLinkResponse>, ApiErr> {
    let provider = find_provider(&config, &provider_id)?;

    if config.jwt_secret.is_empty() {
        return Err(ApiErr::internal("JWT_SECRET not configured"));
    }

    // Check if already linked (one-off query specific to this flow)
    let conn = db.conn();
    let already: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM oauth_identities WHERE user_id = ?1 AND provider = ?2",
            rusqlite::params![user.user_id, provider_id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if already {
        return Err(ApiErr::conflict(format!(
            "{} account already linked",
            provider.display_name
        )));
    }

    let state = crypto::generate_token();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    sq_execute(
        &conn,
        dbq::oauth::insert_state(&state, &provider_id, &expires_at, Some(&user.user_id)),
    )
    .map_err(ApiErr::from_db("oauth state insert for link"))?;

    let redirect_uri = format!(
        "{}/api/auth/oauth/{}/callback",
        config.base_url, provider_id
    );
    let url = oauth::build_authorize_url(provider, &redirect_uri, &state);

    Ok(Json(OAuthLinkResponse { url }))
}
