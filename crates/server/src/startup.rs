use axum::{
    Router,
    extract::{DefaultBodyLimit, FromRef},
    http::{
        HeaderName, HeaderValue, Method,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{delete, get, post, put},
};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::storage::Db;
use crate::{AppConfig, app_config::load_server_bootstrap, routes, storage};

/// Application state shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: AppConfig,
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

pub async fn run() -> anyhow::Result<()> {
    initialize_tracing();

    let bootstrap = load_server_bootstrap();
    let data_dir = bootstrap.data_dir;
    let web_dir = bootstrap.web_dir;
    let port = bootstrap.port;
    let config = bootstrap.config;

    tracing::info!("data directory: {}", data_dir.display());

    let db = storage::init_db(&data_dir)?;
    tracing::info!("database initialized");

    warn_on_disabled_capabilities(&config);

    let base_url = config.base_url.clone();
    let state = AppState { db, config };
    let app = build_app_router(state, &web_dir);

    tracing::info!("starting server at {base_url}");

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn initialize_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "opensession_server=info,tower_http=info".into()),
        )
        .init();
}

fn warn_on_disabled_capabilities(config: &AppConfig) {
    if config.jwt_secret.is_empty() {
        tracing::warn!("JWT_SECRET not set — JWT auth and OAuth will be disabled");
    }
    if config.admin_key.is_empty() {
        tracing::warn!("OPENSESSION_ADMIN_KEY not set — /api/admin routes will return 401");
    }
    if !config.public_feed_enabled {
        tracing::info!(
            "public feed is disabled ({}=false)",
            opensession_api::deploy::ENV_PUBLIC_FEED_ENABLED
        );
    }
}

fn build_app_router(state: AppState, web_dir: &std::path::Path) -> Router {
    let mut app = Router::new()
        .nest("/api", build_api_router())
        .route("/docs", get(routes::docs::handle))
        .route("/llms.txt", get(routes::docs::llms_txt));

    if web_dir.exists() {
        tracing::info!("serving static files from {}", web_dir.display());
        let index_html = web_dir.join("index.html");
        app = app.fallback_service(ServeDir::new(web_dir).fallback(ServeFile::new(index_html)));
    }

    app.layer(TraceLayer::new_for_http())
        .layer(build_cors_layer(&state.config.allowed_origins))
        .with_state(state)
}

fn build_api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(routes::health::health))
        .route("/capabilities", get(routes::capabilities::capabilities))
        .route("/parse/preview", post(routes::ingest::preview))
        .route(
            "/review/local/{review_id}",
            get(routes::review::get_local_review_bundle),
        )
        .route("/auth/verify", post(routes::auth::verify))
        .route("/auth/me", get(routes::auth::me))
        .route("/auth/api-keys/issue", post(routes::auth::issue_api_key))
        .route(
            "/auth/git-credentials",
            get(routes::auth::list_git_credentials).post(routes::auth::create_git_credential),
        )
        .route(
            "/auth/git-credentials/{id}",
            delete(routes::auth::delete_git_credential),
        )
        .route("/auth/register", post(routes::auth::auth_register))
        .route("/auth/login", post(routes::auth::login))
        .route("/auth/refresh", post(routes::auth::refresh))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/auth/password", put(routes::auth::change_password))
        .route("/auth/providers", get(routes::oauth::providers))
        .route("/auth/oauth/{provider}", get(routes::oauth::redirect))
        .route(
            "/auth/oauth/{provider}/callback",
            get(routes::oauth::callback),
        )
        .route("/auth/oauth/{provider}/link", post(routes::oauth::link))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .route("/sessions", get(routes::sessions::list_sessions))
        .route("/sessions/repos", get(routes::sessions::list_session_repos))
        .route("/sessions/{id}", get(routes::sessions::get_session))
        .route("/sessions/{id}/raw", get(routes::sessions::get_session_raw))
        .route(
            "/admin/sessions/{id}",
            delete(routes::admin::delete_session),
        )
}

fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let csrf_header = HeaderName::from_static("x-csrf-token");
    let origin_values: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();

    let mut cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION, csrf_header])
        .allow_credentials(true);
    if !origin_values.is_empty() {
        cors = cors.allow_origin(origin_values);
    }
    cors
}
