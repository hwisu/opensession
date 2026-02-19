use anyhow::{bail, Result};
use std::time::Duration;

use crate::config::load_config;
use opensession_api_client::ApiClient;

/// Check server health status
pub async fn run_status() -> Result<()> {
    let config = load_config()?;
    let client = ApiClient::new(&config.server.url, Duration::from_secs(5))?;

    match client.health().await {
        Ok(resp) => {
            println!(
                "Server: online (v{})  URL: {}",
                resp.version, config.server.url
            );
        }
        Err(e) => {
            println!("Server: offline  URL: {}  Error: {}", config.server.url, e);
        }
    }

    Ok(())
}

/// Verify API key authentication
pub async fn run_verify() -> Result<()> {
    let config = load_config()?;
    if config.server.api_key.trim().is_empty() {
        bail!("API key not configured. Run: opensession account connect --api-key <key>");
    }

    let mut client = ApiClient::new(&config.server.url, Duration::from_secs(5))?;
    client.set_auth(config.server.api_key);

    match client.verify().await {
        Ok(resp) => {
            println!(
                "Authenticated as: {} (user_id: {})",
                resp.nickname, resp.user_id
            );
        }
        Err(e) => {
            println!("Authentication failed: {}", e);
        }
    }

    Ok(())
}
