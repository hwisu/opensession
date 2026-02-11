use opensession_api_client::ApiClient;
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

    let mut api = match ApiClient::new(&server_url, Duration::from_secs(30)) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create pull sync client: {e}");
            return;
        }
    };
    api.set_auth(api_key);

    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = do_pull(&api, &team_id, &db).await {
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

async fn do_pull(api: &ApiClient, team_id: &str, db: &LocalDb) -> anyhow::Result<()> {
    let cursor = db.get_sync_cursor(team_id)?;

    let pull = api.sync_pull(team_id, cursor.as_deref(), Some(100)).await?;
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
