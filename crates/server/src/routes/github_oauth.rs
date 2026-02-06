use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use uuid::Uuid;

use crate::storage::Db;

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    code: String,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(serde::Deserialize)]
struct GitHubUser {
    id: i64,
    login: String,
    avatar_url: Option<String>,
    email: Option<String>,
}

fn github_config() -> Option<(String, String, String)> {
    let client_id = std::env::var("GITHUB_CLIENT_ID").ok()?;
    let client_secret = std::env::var("GITHUB_CLIENT_SECRET").ok()?;
    let base_url =
        std::env::var("OPENSESSION_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    Some((client_id, client_secret, base_url))
}

/// GET /api/auth/github — redirect to GitHub OAuth
pub async fn github_login() -> Response {
    let Some((client_id, _, base_url)) = github_config() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "GitHub OAuth not configured"})),
        )
            .into_response();
    };

    let redirect_uri = format!("{base_url}/api/auth/github/callback");
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={client_id}&redirect_uri={redirect_uri}&scope=read:user,user:email"
    );

    Redirect::temporary(&url).into_response()
}

/// GET /api/auth/github/callback?code=... — exchange code for token, upsert user
pub async fn github_callback(
    State(db): State<Db>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let Some((client_id, client_secret, base_url)) = github_config() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "GitHub OAuth not configured"})),
        )
            .into_response();
    };

    // Exchange code for access token
    let client = reqwest::Client::new();
    let token_res = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": q.code,
        }))
        .send()
        .await;

    let token = match token_res {
        Ok(res) => match res.json::<TokenResponse>().await {
            Ok(t) => t.access_token,
            Err(e) => {
                tracing::error!("parse token response: {e}");
                return Redirect::temporary(&format!("{base_url}/settings?error=oauth_failed"))
                    .into_response();
            }
        },
        Err(e) => {
            tracing::error!("token exchange request: {e}");
            return Redirect::temporary(&format!("{base_url}/settings?error=oauth_failed"))
                .into_response();
        }
    };

    // Fetch GitHub user profile
    let user_res = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "opensession-server")
        .send()
        .await;

    let gh_user = match user_res {
        Ok(res) => match res.json::<GitHubUser>().await {
            Ok(u) => u,
            Err(e) => {
                tracing::error!("parse github user: {e}");
                return Redirect::temporary(&format!("{base_url}/settings?error=oauth_failed"))
                    .into_response();
            }
        },
        Err(e) => {
            tracing::error!("github user request: {e}");
            return Redirect::temporary(&format!("{base_url}/settings?error=oauth_failed"))
                .into_response();
        }
    };

    // Upsert user in DB
    let conn = db.conn();

    // Check if user with this github_id exists
    let existing: Option<String> = conn
        .query_row(
            "SELECT api_key FROM users WHERE github_id = ?1",
            [gh_user.id],
            |row| row.get(0),
        )
        .ok();

    let api_key = if let Some(key) = existing {
        // Update existing user's profile
        let _ = conn.execute(
            "UPDATE users SET github_login = ?1, avatar_url = ?2, email = ?3 WHERE github_id = ?4",
            rusqlite::params![&gh_user.login, &gh_user.avatar_url, &gh_user.email, gh_user.id],
        );
        key
    } else {
        // Create new user
        let user_id = Uuid::new_v4().to_string();
        let api_key = format!("osk_{}", Uuid::new_v4().simple());

        // Try inserting — nickname might conflict, so append a suffix if needed
        let nickname = gh_user.login.clone();
        let result = conn.execute(
            "INSERT INTO users (id, nickname, api_key, github_id, github_login, avatar_url, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![&user_id, &nickname, &api_key, gh_user.id, &gh_user.login, &gh_user.avatar_url, &gh_user.email],
        );

        if result.is_err() {
            // Nickname conflict — use github_login with suffix
            let nickname = format!("{}_{}", gh_user.login, &user_id[..8]);
            let _ = conn.execute(
                "INSERT INTO users (id, nickname, api_key, github_id, github_login, avatar_url, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![&user_id, &nickname, &api_key, gh_user.id, &gh_user.login, &gh_user.avatar_url, &gh_user.email],
            );
        }

        api_key
    };

    // Redirect to frontend callback with api_key
    Redirect::temporary(&format!("{base_url}/auth/callback?api_key={api_key}")).into_response()
}
