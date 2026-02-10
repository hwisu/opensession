use opensession_api_types::SyncPullResponse;
use opensession_local_db::LocalDb;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};

/// Periodically pull team sessions from the server into the local DB.
pub async fn run_pull_sync(
    server_url: String,
    api_key: String,
    team_id: String,
    db: Arc<LocalDb>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    if team_id.is_empty() || api_key.is_empty() {
        debug!("Pull sync disabled: team_id or api_key not configured");
        return;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let pull_url = format!("{}/api/sync/pull", server_url.trim_end_matches('/'));
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = do_pull(&client, &pull_url, &api_key, &team_id, &db).await {
                    error!("Pull sync error: {e:#}");
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("Pull sync shutting down");
                    break;
                }
            }
        }
    }
}

async fn do_pull(
    client: &reqwest::Client,
    pull_url: &str,
    api_key: &str,
    team_id: &str,
    db: &LocalDb,
) -> anyhow::Result<()> {
    let cursor = db.get_sync_cursor(team_id)?;

    let mut query = vec![
        ("team_id", team_id.to_string()),
        ("limit", "100".to_string()),
    ];
    if let Some(ref since) = cursor {
        query.push(("since", since.clone()));
    }

    let resp = client
        .get(pull_url)
        .bearer_auth(api_key)
        .query(&query)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("pull sync HTTP {status}: {body}");
    }

    let pull: SyncPullResponse = resp.json().await?;
    let count = pull.sessions.len();

    for session in &pull.sessions {
        db.upsert_remote_session(session)?;
    }

    if let Some(ref next) = pull.next_cursor {
        db.set_sync_cursor(team_id, next)?;
    }

    if count > 0 {
        info!("Pull sync: received {count} sessions from server");
    } else {
        debug!("Pull sync: up to date");
    }

    Ok(())
}
