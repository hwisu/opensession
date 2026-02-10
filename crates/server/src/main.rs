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
        // Sessions
        .route("/sessions", post(routes::sessions::upload_session))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024)) // 256MB for large sessions
        .route("/sessions", get(routes::sessions::list_sessions))
        .route("/sessions/{id}", get(routes::sessions::get_session))
        .route("/sessions/{id}/raw", get(routes::sessions::get_session_raw))
        // Teams
        .route("/teams", post(routes::teams::create_team))
        .route("/teams", get(routes::teams::list_my_teams))
        .route("/teams/{id}", get(routes::teams::get_team))
        .route("/teams/{id}", put(routes::teams::update_team))
        // Sync
        .route("/sync/pull", get(routes::sync::pull))
        // Team members
        .route("/teams/{id}/members", get(routes::teams::list_members))
        .route("/teams/{id}/members", post(routes::teams::add_member))
        .route(
            "/teams/{team_id}/members/{user_id}",
            delete(routes::teams::remove_member),
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

    let base_url =
        std::env::var("OPENSESSION_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    tracing::info!("starting server at {base_url}");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
