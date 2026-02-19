use axum::{extract::State, Json};
use opensession_api::CapabilitiesResponse;

use crate::AppConfig;

/// GET /api/capabilities — runtime feature availability.
pub async fn capabilities(State(config): State<AppConfig>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        auth_enabled: !config.jwt_secret.is_empty(),
        upload_enabled: true,
        ingest_preview_enabled: true,
        gh_share_enabled: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_capabilities_enable_ingest_and_gh_share() {
        let config = AppConfig {
            base_url: "http://localhost:3000".to_string(),
            oauth_use_request_host: false,
            jwt_secret: "secret".to_string(),
            oauth_providers: Vec::new(),
            public_feed_enabled: true,
        };

        let Json(caps) = capabilities(State(config)).await;
        assert!(caps.auth_enabled);
        assert!(caps.upload_enabled);
        assert!(caps.ingest_preview_enabled);
        assert!(caps.gh_share_enabled);
    }
}
