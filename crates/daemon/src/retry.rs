use anyhow::{Context, Result};
use std::time::Duration;
use tracing::warn;

/// Retry an HTTP POST with exponential backoff.
/// Retries on 5xx and network errors only. Returns immediately on success or 4xx.
pub async fn retry_upload(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &serde_json::Value,
    max_retries: u32,
) -> Result<reqwest::Response> {
    let max_attempts = max_retries + 1;

    for attempt in 0..max_attempts {
        let mut req = client
            .post(url)
            .header("Content-Type", "application/json");
        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        match req.json(body).send().await {
            Ok(resp) if resp.status().is_server_error() => {
                if attempt + 1 < max_attempts {
                    let status = resp.status();
                    let next_delay = 1u64 << attempt.min(4);
                    warn!(
                        "Upload attempt {}/{} failed (HTTP {}), retrying in {}s...",
                        attempt + 1,
                        max_attempts,
                        status,
                        next_delay
                    );
                    tokio::time::sleep(Duration::from_secs(next_delay)).await;
                } else {
                    return Ok(resp);
                }
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if attempt + 1 < max_attempts {
                    let next_delay = 1u64 << attempt.min(4);
                    warn!(
                        "Upload attempt {}/{} failed ({}), retrying in {}s...",
                        attempt + 1,
                        max_attempts,
                        e,
                        next_delay
                    );
                    tokio::time::sleep(Duration::from_secs(next_delay)).await;
                } else {
                    return Err(e).context("Failed to connect to server after retries");
                }
            }
        }
    }

    unreachable!()
}
