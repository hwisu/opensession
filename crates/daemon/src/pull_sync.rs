use opensession_api_client::ApiClient;
use opensession_local_db::{LocalDb, RemoteSessionSummary};
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
        let summary = RemoteSessionSummary {
            id: session.id.clone(),
            user_id: session.user_id.clone(),
            nickname: session.nickname.clone(),
            team_id: session.team_id.clone(),
            tool: session.tool.clone(),
            agent_provider: session.agent_provider.clone(),
            agent_model: session.agent_model.clone(),
            title: session.title.clone(),
            description: session.description.clone(),
            tags: session.tags.clone(),
            created_at: session.created_at.clone(),
            uploaded_at: session.uploaded_at.clone(),
            message_count: session.message_count,
            task_count: session.task_count,
            event_count: session.event_count,
            duration_seconds: session.duration_seconds,
            total_input_tokens: session.total_input_tokens,
            total_output_tokens: session.total_output_tokens,
            git_remote: session.git_remote.clone(),
            git_branch: session.git_branch.clone(),
            git_commit: session.git_commit.clone(),
            git_repo_name: session.git_repo_name.clone(),
            pr_number: session.pr_number,
            pr_url: session.pr_url.clone(),
            working_directory: session.working_directory.clone(),
            files_modified: session.files_modified.clone(),
            files_read: session.files_read.clone(),
            has_errors: session.has_errors,
            max_active_agents: session.max_active_agents,
        };
        db.upsert_remote_session(&summary)?;
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
