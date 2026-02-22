use opensession_api::CapabilitiesResponse;
use worker::*;

use crate::config::WorkerConfig;

fn capabilities_from_config(config: &WorkerConfig) -> CapabilitiesResponse {
    CapabilitiesResponse {
        auth_enabled: config.auth_enabled(),
        parse_preview_enabled: false,
        register_targets: vec!["local".to_string(), "git".to_string()],
        share_modes: vec!["web".to_string(), "git".to_string(), "json".to_string()],
    }
}

pub async fn handle(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    Response::from_json(&capabilities_from_config(&config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_capabilities_enable_key_based_upload_only() {
        let config = WorkerConfig {
            base_url: None,
            jwt_secret: "secret".to_string(),
            oauth_providers: Vec::new(),
        };

        let caps = capabilities_from_config(&config);
        assert!(caps.auth_enabled);
        assert!(!caps.parse_preview_enabled);
        assert_eq!(caps.register_targets, vec!["local", "git"]);
        assert_eq!(caps.share_modes, vec!["web", "git", "json"]);
    }
}
