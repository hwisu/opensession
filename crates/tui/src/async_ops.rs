use std::time::Duration;

use opensession_api::*;

use crate::app::TeamInfo;
use crate::config::DaemonConfig;

/// Commands that require async I/O (network calls).
pub enum AsyncCommand {
    // ── Auth ──────────────────────────────────────────────────────────
    Login {
        email: String,
        password: String,
    },

    // ── Upload flow (existing) ────────────────────────────────────────
    FetchUploadTeams,
    UploadSession {
        session_json: serde_json::Value,
        team_id: Option<String>,
        team_name: String,
        body_url: Option<String>,
    },

    // ── Teams ─────────────────────────────────────────────────────────
    FetchTeams,
    FetchTeamDetail(String),
    FetchMembers(String),
    CreateTeam {
        name: String,
    },
    InviteMember {
        team_id: String,
        email: String,
    },
    RemoveMember {
        team_id: String,
        user_id: String,
    },

    // ── Invitations ───────────────────────────────────────────────────
    FetchInvitations,
    AcceptInvitation(String),
    DeclineInvitation(String),

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
    // Auth
    Login(Result<(String, String), String>), // (api_key, nickname)

    // Upload flow
    UploadTeams(Result<Vec<TeamInfo>, String>),
    UploadDone(Result<(String, String), (String, String)>), // Ok((team_name, url)) or Err((team_name, error))

    // Teams
    Teams(Result<Vec<TeamResponse>, String>),
    TeamDetail(Result<TeamDetailResponse, String>),
    Members(Result<Vec<MemberResponse>, String>),

    // Invitations
    Invitations(Result<Vec<InvitationResponse>, String>),

    // Profile / Account
    Profile(Result<UserSettingsResponse, String>),
    ApiKey(Result<RegenerateKeyResponse, String>),

    // Server Sessions
    ServerSessions(Result<SessionListResponse, String>),

    // Generic OK (team create, invite, remove, password change, accept/decline)
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
        // ── Login ─────────────────────────────────────────────────────
        AsyncCommand::Login { email, password } => {
            let result = async {
                let mut client = opensession_api_client::ApiClient::new(
                    &config.server.url,
                    Duration::from_secs(10),
                )
                .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

                let tokens = client
                    .login(&LoginRequest { email, password })
                    .await
                    .map_err(|e| format!("{e}"))?;

                client.set_auth(tokens.access_token);
                let user = client.me().await.map_err(|e| format!("{e}"))?;
                Ok((user.api_key, user.nickname))
            }
            .await;
            CommandResult::Login(result)
        }

        // ── Upload flow ───────────────────────────────────────────────
        AsyncCommand::FetchUploadTeams => {
            let result = async {
                let client = make_client(config)?;
                let list = client.list_teams().await.map_err(|e| format!("{e}"))?;

                let mut teams = vec![TeamInfo {
                    id: String::new(),
                    name: "Personal (Public)".to_string(),
                    is_personal: true,
                }];
                for t in list.teams {
                    teams.push(TeamInfo {
                        id: t.id,
                        name: t.name,
                        is_personal: false,
                    });
                }
                Ok(teams)
            }
            .await;
            CommandResult::UploadTeams(result)
        }

        AsyncCommand::UploadSession {
            session_json,
            team_id,
            team_name,
            body_url,
        } => {
            let result = async {
                let client = make_client(config)?;
                let session: opensession_core::trace::Session =
                    serde_json::from_value(session_json).map_err(|e| format!("parse: {e}"))?;
                let resp = client
                    .upload_session(&UploadRequest {
                        session,
                        team_id,
                        body_url,
                        linked_session_ids: None,
                        git_remote: None,
                        git_branch: None,
                        git_commit: None,
                        git_repo_name: None,
                        pr_number: None,
                        pr_url: None,
                    })
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(resp.url)
            }
            .await;
            CommandResult::UploadDone(match result {
                Ok(url) => Ok((team_name, url)),
                Err(e) => Err((team_name, e)),
            })
        }

        // ── Teams ─────────────────────────────────────────────────────
        AsyncCommand::FetchTeams => {
            let result = async {
                let client = make_client(config)?;
                let list = client.list_teams().await.map_err(|e| format!("{e}"))?;
                Ok(list.teams)
            }
            .await;
            CommandResult::Teams(result)
        }

        AsyncCommand::FetchTeamDetail(id) => {
            let result = async {
                let client = make_client(config)?;
                client.get_team(&id).await.map_err(|e| format!("{e}"))
            }
            .await;
            CommandResult::TeamDetail(result)
        }

        AsyncCommand::FetchMembers(team_id) => {
            let result = async {
                let client = make_client(config)?;
                let list = client
                    .list_members(&team_id)
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(list.members)
            }
            .await;
            CommandResult::Members(result)
        }

        AsyncCommand::CreateTeam { name } => {
            let result = async {
                let client = make_client(config)?;
                let resp = client
                    .create_team(&CreateTeamRequest {
                        name: name.clone(),
                        description: None,
                        is_public: Some(false),
                    })
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(format!("Team '{}' created", resp.name))
            }
            .await;
            CommandResult::GenericOk(result)
        }

        AsyncCommand::InviteMember { team_id, email } => {
            let result = async {
                let client = make_client(config)?;
                client
                    .invite_member(
                        &team_id,
                        &InviteRequest {
                            email: Some(email.clone()),
                            oauth_provider: None,
                            oauth_provider_username: None,
                            role: None,
                        },
                    )
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(format!("Invitation sent to {email}"))
            }
            .await;
            CommandResult::GenericOk(result)
        }

        AsyncCommand::RemoveMember { team_id, user_id } => {
            let result = async {
                let client = make_client(config)?;
                client
                    .remove_member(&team_id, &user_id)
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok("Member removed".to_string())
            }
            .await;
            CommandResult::GenericOk(result)
        }

        // ── Invitations ───────────────────────────────────────────────
        AsyncCommand::FetchInvitations => {
            let result = async {
                let client = make_client(config)?;
                let list = client
                    .list_invitations()
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(list.invitations)
            }
            .await;
            CommandResult::Invitations(result)
        }

        AsyncCommand::AcceptInvitation(id) => {
            let result = async {
                let client = make_client(config)?;
                let resp = client
                    .accept_invitation(&id)
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok(format!("Joined team (role: {})", resp.role))
            }
            .await;
            CommandResult::GenericOk(result)
        }

        AsyncCommand::DeclineInvitation(id) => {
            let result = async {
                let client = make_client(config)?;
                client
                    .decline_invitation(&id)
                    .await
                    .map_err(|e| format!("{e}"))?;
                Ok("Invitation declined".to_string())
            }
            .await;
            CommandResult::GenericOk(result)
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
                client.regenerate_key().await.map_err(|e| format!("{e}"))
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
