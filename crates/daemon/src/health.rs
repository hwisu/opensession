use opensession_api_client::ApiClient;
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

    let mut api = match ApiClient::new(&server_url, Duration::from_secs(10)) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to create health check client: {e}");
            return;
        }
    };
    if !api_key.is_empty() {
        api.set_auth(api_key);
    }

    loop {
        tokio::select! {
            _ = interval.tick() => {
                check_server(&api).await;
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

async fn check_server(api: &ApiClient) {
    match api.health().await {
        Ok(_) => debug!("Health check OK: server reachable"),
        Err(e) => warn!("Health check: server issue ({e})"),
    }

    if api.auth_token().is_some() {
        match api.verify().await {
            Ok(_) => debug!("Health check OK: API key valid"),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("401") {
                    warn!("Health check: API key is invalid or expired");
                }
                // else: server unreachable — already warned above
            }
        }
    }
}

fn check_watch_paths(watch_paths: &[PathBuf]) {
    for path in watch_paths {
        if !path.exists() {
            warn!(
                "Health check: watch path no longer exists: {}",
                path.display()
            );
        }
    }
}
