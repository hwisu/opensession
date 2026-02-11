use axum::Json;
use opensession_api_types::HealthResponse;

/// GET /api/health — server liveness check.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
