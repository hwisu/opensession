use std::time::Duration;

use anyhow::{Context, Result};
use tracing::warn;

/// Configuration for retry behaviour on upload-style POST requests.
pub struct RetryConfig {
    pub max_retries: usize,
    pub delays: Vec<u64>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            delays: vec![1, 2, 4],
        }
    }
}

/// Retry an HTTP POST with exponential backoff.
///
/// Retries on network errors and 5xx responses.
/// Returns immediately on success or 4xx.
pub async fn retry_post(
    client: &reqwest::Client,
    url: &str,
    auth_token: Option<&str>,
    body: &serde_json::Value,
    config: &RetryConfig,
) -> Result<reqwest::Response> {
    let max_attempts = config.max_retries + 1;

    for attempt in 0..max_attempts {
        let mut req = client.post(url).header("Content-Type", "application/json");
        if let Some(token) = auth_token {
            req = req.bearer_auth(token);
        }

        match req.json(body).send().await {
            Ok(resp) if resp.status().is_server_error() => {
                if attempt < config.delays.len() {
                    let status = resp.status();
                    warn!(
                        "POST attempt {}/{} failed (HTTP {}), retrying in {}s…",
                        attempt + 1,
                        max_attempts,
                        status,
                        config.delays[attempt],
                    );
                    tokio::time::sleep(Duration::from_secs(config.delays[attempt])).await;
                } else {
                    return Ok(resp);
                }
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if attempt < config.delays.len() {
                    warn!(
                        "POST attempt {}/{} failed ({}), retrying in {}s…",
                        attempt + 1,
                        max_attempts,
                        e,
                        config.delays[attempt],
                    );
                    tokio::time::sleep(Duration::from_secs(config.delays[attempt])).await;
                } else {
                    return Err(e).context("Failed to connect after retries");
                }
            }
        }
    }

    unreachable!()
}
