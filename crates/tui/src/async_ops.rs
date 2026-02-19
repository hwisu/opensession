use std::time::Duration;

use opensession_api::{
    ChangePasswordRequest, IssueApiKeyResponse, SessionListQuery, SessionListResponse,
    UploadRequest, UserSettingsResponse,
};

use crate::app::TeamInfo;
use crate::config::DaemonConfig;

/// Commands that require async I/O (network calls).
pub enum AsyncCommand {
    // ── Upload flow (existing) ────────────────────────────────────────
    FetchUploadTeams,
    UploadSession {
        session_json: serde_json::Value,
        target_name: String,
        body_url: Option<String>,
    },

    // ── Profile / Account ─────────────────────────────────────────────
    FetchProfile,
    ChangePassword {
        current: String,
        new_password: String,
    },
    RegenerateApiKey,

    // ── Server Sessions ───────────────────────────────────────────────
    #[allow(dead_code)]
    FetchServerSessions(SessionListQuery),

    // ── Delete ────────────────────────────────────────────────────────
    DeleteSession {
        session_id: String,
    },
}

/// Results returned by async commands.
pub enum CommandResult {
    // Upload flow
    UploadTeams(Result<Vec<TeamInfo>, String>),
    UploadDone(Result<(String, String), (String, String)>), // Ok((target_name, url)) or Err((target_name, error))

    // Profile / Account
    Profile(Result<UserSettingsResponse, String>),
    ApiKey(Result<IssueApiKeyResponse, String>),

    // Server Sessions
    ServerSessions(Result<SessionListResponse, String>),

    // Generic OK (password change etc.)
    GenericOk(Result<String, String>),

    // Delete
    DeleteSession(Result<String, String>), // Ok(session_id) or Err(msg)
}

fn make_client(config: &DaemonConfig) -> Result<opensession_api_client::ApiClient, String> {
    let mut client =
        opensession_api_client::ApiClient::new(&config.server.url, Duration::from_secs(15))
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
    client.set_auth(config.server.api_key.clone());
    Ok(client)
}

pub async fn execute(cmd: AsyncCommand, config: &DaemonConfig) -> CommandResult {
    match cmd {
        // ── Upload flow ───────────────────────────────────────────────
        AsyncCommand::FetchUploadTeams => {
            let result = async {
                Ok(vec![TeamInfo {
                    id: String::new(),
                    name: "Personal (Public)".to_string(),
                    is_personal: true,
                }])
            }
            .await;
            CommandResult::UploadTeams(result)
        }

        AsyncCommand::UploadSession {
            session_json,
            target_name,
            body_url,
        } => {
            let result = async {
                let client = make_client(config)?;
                let session: opensession_core::trace::Session =
                    serde_json::from_value(session_json).map_err(|e| format!("parse: {e}"))?;
                let resp = client
                    .upload_session(&UploadRequest {
                        session,
                        body_url,
                        linked_session_ids: None,
                        git_remote: None,
                        git_branch: None,
                        git_commit: None,
                        git_repo_name: None,
                        pr_number: None,
                        pr_url: None,
                        score_plugin: None,
                    })
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(resp.url)
            }
            .await;
            CommandResult::UploadDone(match result {
                Ok(url) => Ok((target_name, url)),
                Err(e) => Err((target_name, e)),
            })
        }

        // ── Profile / Account ─────────────────────────────────────────
        AsyncCommand::FetchProfile => {
            let result = async {
                let client = make_client(config)?;
                client.me().await.map_err(|e| format!("{e}"))
            }
            .await;
            CommandResult::Profile(result)
        }

        AsyncCommand::ChangePassword {
            current,
            new_password,
        } => {
            let result = async {
                let client = make_client(config)?;
                client
                    .change_password(&ChangePasswordRequest {
                        current_password: current,
                        new_password,
                    })
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok("Password changed".to_string())
            }
            .await;
            CommandResult::GenericOk(result)
        }

        AsyncCommand::RegenerateApiKey => {
            let result = async {
                let client = make_client(config)?;
                client.issue_api_key().await.map_err(|e| format!("{e}"))
            }
            .await;
            CommandResult::ApiKey(result)
        }

        // ── Delete ─────────────────────────────────────────────────────
        AsyncCommand::DeleteSession { session_id } => {
            let result = async {
                let client = make_client(config)?;
                client
                    .delete_session(&session_id)
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(session_id)
            }
            .await;
            CommandResult::DeleteSession(result)
        }

        // ── Server Sessions ───────────────────────────────────────────
        AsyncCommand::FetchServerSessions(query) => {
            let result = async {
                let client = make_client(config)?;
                client
                    .list_sessions(&query)
                    .await
                    .map_err(|e| format!("{e}"))
            }
            .await;
            CommandResult::ServerSessions(result)
        }
    }
}
