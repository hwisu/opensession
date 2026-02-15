mod error;
mod routes;
mod storage;

use axum::{
    extract::{DefaultBodyLimit, FromRef},
    routing::{delete, get, post, put},
    Router,
};
use std::path::PathBuf;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use opensession_api::oauth::{self, OAuthProviderConfig};
use storage::Db;

/// Application state shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: AppConfig,
}

/// Server configuration loaded from environment variables.
#[derive(Clone)]
pub struct AppConfig {
    pub base_url: String,
    pub oauth_use_request_host: bool,
    pub jwt_secret: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
}

impl FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AppState> for AppConfig {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

/// Load OAuth providers from environment variables.
fn load_oauth_providers() -> Vec<OAuthProviderConfig> {
    [try_load_github(), try_load_gitlab()]
        .into_iter()
        .flatten()
        .collect()
}

fn try_load_github() -> Option<OAuthProviderConfig> {
    let id = std::env::var("GITHUB_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let secret = std::env::var("GITHUB_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    tracing::info!("OAuth provider enabled: GitHub");
    Some(oauth::github_preset(id, secret))
}

fn try_load_gitlab() -> Option<OAuthProviderConfig> {
    let url = std::env::var("GITLAB_URL").ok().filter(|s| !s.is_empty())?;
    let id = std::env::var("GITLAB_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let secret = std::env::var("GITLAB_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())?;
    let ext_url = std::env::var("GITLAB_EXTERNAL_URL")
        .ok()
        .filter(|s| !s.is_empty());
    tracing::info!("OAuth provider enabled: GitLab ({})", url);
    Some(oauth::gitlab_preset(url, ext_url, id, secret))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "opensession_server=info,tower_http=info".into()),
        )
        .init();

    // Data directory
    let data_dir = std::env::var("OPENSESSION_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));

    tracing::info!("data directory: {}", data_dir.display());

    // Initialize database
    let db = storage::init_db(&data_dir)?;
    tracing::info!("database initialized");

    let base_url_env = std::env::var("BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("OPENSESSION_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty())
        });
    let base_url = base_url_env
        .clone()
        .unwrap_or_else(|| "http://localhost:3000".into());

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_default();
    if jwt_secret.is_empty() {
        tracing::warn!("JWT_SECRET not set — JWT auth and OAuth will be disabled");
    }

    let oauth_providers = load_oauth_providers();

    let config = AppConfig {
        base_url: base_url.clone(),
        oauth_use_request_host: base_url_env.is_none(),
        jwt_secret,
        oauth_providers,
    };

    let state = AppState { db, config };

    // Build API routes
    let api = Router::new()
        // Health
        .route("/health", get(routes::health::health))
        // Auth (legacy)
        .route("/register", post(routes::auth::register))
        .route("/auth/verify", post(routes::auth::verify))
        .route("/auth/me", get(routes::auth::me))
        .route("/auth/regenerate-key", post(routes::auth::regenerate_key))
        // Auth (email/password + JWT)
        .route("/auth/register", post(routes::auth::auth_register))
        .route("/auth/login", post(routes::auth::login))
        .route("/auth/refresh", post(routes::auth::refresh))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/auth/password", put(routes::auth::change_password))
        // OAuth (generic — handles any provider)
        .route("/auth/providers", get(routes::oauth::providers))
        .route("/auth/oauth/{provider}", get(routes::oauth::redirect))
        .route(
            "/auth/oauth/{provider}/callback",
            get(routes::oauth::callback),
        )
        .route("/auth/oauth/{provider}/link", post(routes::oauth::link))
        // Sessions
        .route("/sessions", post(routes::sessions::upload_session))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024)) // 256MB for large sessions
        .route("/sessions", get(routes::sessions::list_sessions))
        .route(
            "/sessions/{id}",
            get(routes::sessions::get_session).delete(routes::sessions::delete_session),
        )
        .route("/sessions/{id}/raw", get(routes::sessions::get_session_raw))
        // Teams
        .route("/teams", post(routes::teams::create_team))
        .route("/teams", get(routes::teams::list_my_teams))
        .route("/teams/{id}/stats", get(routes::teams::team_stats))
        .route("/teams/{id}", get(routes::teams::get_team))
        .route("/teams/{id}", put(routes::teams::update_team))
        .route(
            "/teams/{id}/keys",
            post(routes::teams::create_team_invite_key),
        )
        .route(
            "/teams/{id}/keys",
            get(routes::teams::list_team_invite_keys),
        )
        .route(
            "/teams/{id}/keys/{key_id}",
            delete(routes::teams::revoke_team_invite_key),
        )
        .route(
            "/teams/join-with-key",
            post(routes::teams::join_team_with_key),
        )
        // Sync
        .route("/sync/pull", get(routes::sync::pull))
        // Team members
        .route("/teams/{id}/members", get(routes::teams::list_members))
        .route("/teams/{id}/members", post(routes::teams::add_member))
        .route(
            "/teams/{id}/invitations",
            get(routes::teams::list_team_invitations),
        )
        .route(
            "/teams/{id}/invitations/{invitation_id}",
            delete(routes::teams::cancel_team_invitation),
        )
        .route(
            "/teams/{team_id}/members/{user_id}",
            delete(routes::teams::remove_member),
        )
        // Invitations
        .route("/teams/{id}/invite", post(routes::teams::invite_member))
        .route("/invitations", get(routes::teams::list_invitations))
        .route(
            "/invitations/{id}/accept",
            post(routes::teams::accept_invitation),
        )
        .route(
            "/invitations/{id}/decline",
            post(routes::teams::decline_invitation),
        );

    // Build main router
    let mut app = Router::new()
        .nest("/api", api)
        // Docs (content negotiation: markdown for AI agents, HTML for browsers)
        .route("/docs", get(routes::docs::handle))
        .route("/llms.txt", get(routes::docs::llms_txt));

    // Serve static files from web build if present
    let web_dir = std::env::var("OPENSESSION_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/build"));
    if web_dir.exists() {
        tracing::info!("serving static files from {}", web_dir.display());
        let index_html = web_dir.join("index.html");
        app = app.fallback_service(ServeDir::new(&web_dir).fallback(ServeFile::new(index_html)));
    }

    let app = app
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    tracing::info!("starting server at {base_url}");

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
