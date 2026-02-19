use axum::{extract::State, Json};
use opensession_api::CapabilitiesResponse;

use crate::AppConfig;

/// GET /api/capabilities — runtime feature availability.
pub async fn capabilities(State(config): State<AppConfig>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        auth_enabled: !config.jwt_secret.is_empty(),
        upload_enabled: true,
    })
}
