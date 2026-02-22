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
    pub admin_key: String,
    pub oauth_providers: Vec<OAuthProviderConfig>,
    pub public_feed_enabled: bool,
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

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .and_then(|v| oauth::normalize_oauth_config_value(&v))
}

fn try_load_github() -> Option<OAuthProviderConfig> {
    let id = env_trimmed("GITHUB_CLIENT_ID")?;
    let secret = env_trimmed("GITHUB_CLIENT_SECRET")?;
    tracing::info!("OAuth provider enabled: GitHub");
    Some(oauth::github_preset(id, secret))
}

fn try_load_gitlab() -> Option<OAuthProviderConfig> {
    let url = env_trimmed("GITLAB_URL")?;
    let id = env_trimmed("GITLAB_CLIENT_ID")?;
    let secret = env_trimmed("GITLAB_CLIENT_SECRET")?;
    let ext_url = env_trimmed("GITLAB_EXTERNAL_URL");
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

    let base_url_env = env_trimmed("BASE_URL").or_else(|| env_trimmed("OPENSESSION_BASE_URL"));
    let base_url = base_url_env
        .clone()
        .unwrap_or_else(|| "http://localhost:3000".into());

    let jwt_secret = env_trimmed("JWT_SECRET").unwrap_or_default();
    if jwt_secret.is_empty() {
        tracing::warn!("JWT_SECRET not set — JWT auth and OAuth will be disabled");
    }
    let admin_key = env_trimmed("OPENSESSION_ADMIN_KEY").unwrap_or_default();
    if admin_key.is_empty() {
        tracing::warn!("OPENSESSION_ADMIN_KEY not set — /api/admin routes will return 401");
    }

    let oauth_providers = load_oauth_providers();
    let public_feed_enabled_raw =
        std::env::var(opensession_api::deploy::ENV_PUBLIC_FEED_ENABLED).ok();
    let public_feed_enabled =
        opensession_api::deploy::parse_bool_flag(public_feed_enabled_raw.as_deref(), true);
    if !public_feed_enabled {
        tracing::info!(
            "public feed is disabled ({}=false)",
            opensession_api::deploy::ENV_PUBLIC_FEED_ENABLED
        );
    }

    let config = AppConfig {
        base_url: base_url.clone(),
        oauth_use_request_host: base_url_env.is_none(),
        jwt_secret,
        admin_key,
        oauth_providers,
        public_feed_enabled,
    };

    let state = AppState { db, config };

    // Build API routes
    let api = Router::new()
        // Health
        .route("/health", get(routes::health::health))
        .route("/capabilities", get(routes::capabilities::capabilities))
        .route("/parse/preview", post(routes::ingest::preview))
        // Auth
        .route("/auth/verify", post(routes::auth::verify))
        .route("/auth/me", get(routes::auth::me))
        .route("/auth/api-keys/issue", post(routes::auth::issue_api_key))
        // Auth (email/password + JWT)
        .route("/auth/register", post(routes::auth::auth_register))
        .route("/auth/login", post(routes::auth::login))
        .route("/auth/refresh", post(routes::auth::refresh))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/auth/password", put(routes::auth::change_password))
        // OAuth
        .route("/auth/providers", get(routes::oauth::providers))
        .route("/auth/oauth/{provider}", get(routes::oauth::redirect))
        .route(
            "/auth/oauth/{provider}/callback",
            get(routes::oauth::callback),
        )
        .route("/auth/oauth/{provider}/link", post(routes::oauth::link))
        // Sessions (read-only)
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .route("/sessions", get(routes::sessions::list_sessions))
        .route("/sessions/{id}", get(routes::sessions::get_session))
        .route("/sessions/{id}/raw", get(routes::sessions::get_session_raw))
        // Admin
        .route(
            "/admin/sessions/{id}",
            delete(routes::admin::delete_session),
        );

    // Build main router
    let mut app = Router::new()
        .nest("/api", api)
        // Docs (content negotiation: markdown for AI agents, HTML for browsers)
        .route("/docs", get(routes::docs::handle))
        .route("/llms.txt", get(routes::docs::llms_txt))
        // Removed legacy source routes
        .route("/git", get(routes::legacy::removed_route))
        .route("/gh", get(routes::legacy::removed_route))
        .route("/gh/*path", get(routes::legacy::removed_route))
        .route("/resolve", get(routes::legacy::removed_route))
        .route("/resolve/*path", get(routes::legacy::removed_route));

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
