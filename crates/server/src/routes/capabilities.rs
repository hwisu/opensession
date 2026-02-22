use axum::{extract::State, Json};
use opensession_api::CapabilitiesResponse;

use crate::AppConfig;

/// GET /api/capabilities — runtime feature availability.
pub async fn capabilities(State(config): State<AppConfig>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        auth_enabled: !config.jwt_secret.is_empty(),
        parse_preview_enabled: true,
        register_targets: vec!["local".to_string(), "git".to_string()],
        share_modes: vec!["web".to_string(), "git".to_string(), "json".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_capabilities_enable_parse_and_share_modes() {
        let config = AppConfig {
            base_url: "http://localhost:3000".to_string(),
            oauth_use_request_host: false,
            jwt_secret: "secret".to_string(),
            admin_key: "adminkey".to_string(),
            oauth_providers: Vec::new(),
            public_feed_enabled: true,
        };

        let Json(caps) = capabilities(State(config)).await;
        assert!(caps.auth_enabled);
        assert!(caps.parse_preview_enabled);
        assert_eq!(caps.register_targets, vec!["local", "git"]);
        assert_eq!(caps.share_modes, vec!["web", "git", "json"]);
    }
}
