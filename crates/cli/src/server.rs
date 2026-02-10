use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::config::load_config;

/// Check server health status
pub async fn run_status() -> Result<()> {
    let config = load_config()?;
    let url = format!("{}/api/health", config.server.url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({"status": "ok"}));
            let version = body
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            println!("Server: online (v{})  URL: {}", version, config.server.url);
        }
        Ok(resp) => {
            println!(
                "Server: offline  URL: {}  Error: HTTP {}",
                config.server.url,
                resp.status()
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
    if config.server.api_key.is_empty() {
        bail!("API key not configured. Run: opensession config --api-key <key>");
    }

    let url = format!(
        "{}/api/auth/verify",
        config.server.url.trim_end_matches('/')
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.server.api_key))
        .send()
        .await
        .context("Failed to connect to server")?;

    if response.status().is_success() {
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        let nickname = body
            .get("nickname")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let user_id = body
            .get("user_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        println!("Authenticated as: {} (user_id: {})", nickname, user_id);
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        println!("Authentication failed: HTTP {} - {}", status, body);
    }

    Ok(())
}

/// Retry an async HTTP operation with exponential backoff.
/// Retries on network errors and 5xx responses. Returns the response on success or 4xx.
pub async fn retry_upload(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response> {
    let delays = [1, 2, 4]; // seconds
    let max_attempts = delays.len() + 1;

    for attempt in 0..max_attempts {
        let mut req = client.post(url).header("Content-Type", "application/json");
        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        match req.json(body).send().await {
            Ok(resp) if resp.status().is_server_error() => {
                if attempt < delays.len() {
                    let status = resp.status();
                    tracing::warn!(
                        "Upload attempt {}/{} failed (HTTP {}), retrying in {}s...",
                        attempt + 1,
                        max_attempts,
                        status,
                        delays[attempt]
                    );
                    tokio::time::sleep(Duration::from_secs(delays[attempt])).await;
                } else {
                    return Ok(resp);
                }
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if attempt < delays.len() {
                    tracing::warn!(
                        "Upload attempt {}/{} failed ({}), retrying in {}s...",
                        attempt + 1,
                        max_attempts,
                        e,
                        delays[attempt]
                    );
                    tokio::time::sleep(Duration::from_secs(delays[attempt])).await;
                } else {
                    return Err(e).context("Failed to connect to server after retries");
                }
            }
        }
    }

    unreachable!()
}
