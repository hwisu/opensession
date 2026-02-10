use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Run periodic health checks: server connectivity + watch path validation.
pub async fn run_health_check(
    server_url: String,
    api_key: String,
    watch_paths: Vec<PathBuf>,
    interval_secs: u64,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    if interval_secs == 0 {
        info!("Health checks disabled (interval_secs=0)");
        return;
    }

    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    // Skip the first immediate tick
    interval.tick().await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                check_server(&client, &server_url, &api_key).await;
                check_watch_paths(&watch_paths);
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    debug!("Health check shutting down");
                    break;
                }
            }
        }
    }
}

async fn check_server(client: &reqwest::Client, server_url: &str, api_key: &str) {
    let url = format!("{}/api/health", server_url.trim_end_matches('/'));

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            debug!("Health check OK: server reachable");
        }
        Ok(resp) => {
            warn!("Health check: server returned HTTP {}", resp.status());
        }
        Err(e) => {
            warn!("Health check: server unreachable ({})", e);
        }
    }

    // Verify API key if configured
    if !api_key.is_empty() {
        let verify_url = format!("{}/api/auth/verify", server_url.trim_end_matches('/'));
        match client
            .post(&verify_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                debug!("Health check OK: API key valid");
            }
            Ok(resp) if resp.status().as_u16() == 401 => {
                warn!("Health check: API key is invalid or expired");
            }
            _ => {
                // Server unreachable — already warned above
            }
        }
    }
}

fn check_watch_paths(watch_paths: &[PathBuf]) {
    for path in watch_paths {
        if !path.exists() {
            warn!("Health check: watch path no longer exists: {}", path.display());
        }
    }
}
