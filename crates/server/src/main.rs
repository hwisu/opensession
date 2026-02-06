mod routes;
mod storage;

use axum::{
    extract::{DefaultBodyLimit, FromRef},
    routing::{get, post, put},
    Router,
};
use std::path::PathBuf;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use storage::Db;

/// Application state shared across all handlers.
#[derive(Clone)]
struct AppState {
    db: Db,
}

impl FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
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

    let state = AppState { db };

    // Build API routes
    let api = Router::new()
        // Health
        .route("/health", get(routes::health::health))
        // Auth
        .route("/register", post(routes::auth::register))
        .route("/auth/verify", post(routes::auth::verify))
        .route("/auth/me", get(routes::auth::me))
        .route("/auth/regenerate-key", post(routes::auth::regenerate_key))
        // GitHub OAuth
        .route("/auth/github", get(routes::github_oauth::github_login))
        .route("/auth/github/callback", get(routes::github_oauth::github_callback))
        // Sessions
        .route("/sessions", post(routes::sessions::upload_session))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024)) // 256MB for large sessions
        .route("/sessions", get(routes::sessions::list_sessions))
        .route("/sessions/{id}", get(routes::sessions::get_session))
        .route("/sessions/{id}/raw", get(routes::sessions::get_session_raw))
        // Groups
        .route("/groups", post(routes::groups::create_group))
        .route("/groups", get(routes::groups::list_my_groups))
        .route("/groups/{id}", get(routes::groups::get_group))
        .route("/groups/{id}", put(routes::groups::update_group))
        .route("/groups/{id}/members", get(routes::invites::list_members))
        // Invites
        .route(
            "/groups/{id}/invites",
            post(routes::invites::create_invite),
        )
        .route(
            "/invites/{code}/join",
            post(routes::invites::join_via_invite),
        );

    // Build main router
    let mut app = Router::new().nest("/api", api);

    // Serve static files from web build if present
    let web_dir = std::env::var("OPENSESSION_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/build"));
    if web_dir.exists() {
        tracing::info!("serving static files from {}", web_dir.display());
        let index_html = web_dir.join("index.html");
        app = app.fallback_service(
            ServeDir::new(&web_dir).fallback(ServeFile::new(index_html)),
        );
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

    let base_url =
        std::env::var("OPENSESSION_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    tracing::info!("starting server at {base_url}");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
