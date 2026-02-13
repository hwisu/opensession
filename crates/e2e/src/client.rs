use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tokio::sync::OnceCell;
use uuid::Uuid;

use opensession_api::{
    AddMemberRequest, AuthRegisterRequest, AuthTokenResponse, CreateTeamRequest, LoginRequest,
    TeamResponse, UserSettingsResponse,
};
use opensession_api_client::ApiClient;

/// Holds connection info and shared admin state for a test run.
pub struct TestContext {
    pub api: ApiClient,
    admin: OnceCell<TestUser>,
}

/// A registered test user with credentials.
#[derive(Debug, Clone)]
pub struct TestUser {
    pub user_id: String,
    pub nickname: String,
    pub email: String,
    pub password: String,
    pub api_key: String,
    pub access_token: String,
    pub refresh_token: String,
}

impl TestContext {
    pub fn new(base_url: String) -> Self {
        Self {
            api: ApiClient::with_client(reqwest::Client::new(), &base_url),
            admin: OnceCell::new(),
        }
    }

    /// Build a full API URL from a path like `/health`.
    pub fn url(&self, path: &str) -> String {
        format!("{}/api{}", self.api.base_url(), path)
    }

    /// Register a fresh user with a unique email and nickname.
    pub async fn register_user(&self) -> Result<TestUser> {
        let id = Uuid::new_v4();
        let short = &id.to_string()[..8];
        let email = format!("test-{id}@e2e.local");
        let nickname = format!("e2e-{short}");
        let password = "testpass99".to_string();

        let tokens: AuthTokenResponse = {
            let resp = self
                .api
                .post_json_raw(
                    "/auth/register",
                    &AuthRegisterRequest {
                        email: email.clone(),
                        password: password.clone(),
                        nickname: nickname.clone(),
                    },
                )
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("register failed ({status}): {body}"));
            }
            resp.json().await?
        };

        // Fetch api_key via /auth/me
        let me: UserSettingsResponse = self
            .api
            .get_with_auth("/auth/me", &tokens.access_token)
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(TestUser {
            user_id: tokens.user_id,
            nickname: tokens.nickname,
            email,
            password,
            api_key: me.api_key,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
        })
    }

    /// One-time setup for the first user (used as team creator in tests).
    pub async fn setup_admin(&self) -> Result<&TestUser> {
        self.admin
            .get_or_try_init(|| async { self.register_user().await })
            .await
    }

    /// One-time admin setup for Worker (login existing admin).
    pub async fn setup_admin_with_credentials(
        &self,
        email: &str,
        password: &str,
    ) -> Result<&TestUser> {
        let email = email.to_string();
        let password = password.to_string();
        self.admin
            .get_or_try_init(|| async {
                let resp = self
                    .api
                    .post_json_raw(
                        "/auth/login",
                        &LoginRequest {
                            email: email.clone(),
                            password: password.clone(),
                        },
                    )
                    .await?;

                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(anyhow!("admin login failed ({status}): {body}"));
                }

                let tokens: AuthTokenResponse = resp.json().await?;

                let me: UserSettingsResponse = self
                    .api
                    .get_with_auth("/auth/me", &tokens.access_token)
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;

                Ok(TestUser {
                    user_id: tokens.user_id,
                    nickname: tokens.nickname,
                    email,
                    password,
                    api_key: me.api_key,
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token,
                })
            })
            .await
    }

    /// Get the admin user, if initialized.
    pub fn admin(&self) -> Option<&TestUser> {
        self.admin.get()
    }

    /// Register a user and set up a team with the admin, adding the user as a member.
    /// Returns `(user, team_id)`.
    pub async fn setup_user_with_team(&self) -> Result<(TestUser, String)> {
        let admin = self.admin.get().context("admin not initialized")?;
        let user = self.register_user().await?;

        let team_name = format!("e2e-team-{}", &Uuid::new_v4().to_string()[..8]);
        let resp = self
            .api
            .post_json_with_auth(
                "/teams",
                &admin.access_token,
                &CreateTeamRequest {
                    name: team_name,
                    description: Some("E2E test team".into()),
                    is_public: Some(false),
                },
            )
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("create team failed ({status}): {body}"));
        }
        let team: TeamResponse = resp.json().await?;

        // Admin adds user as member
        let resp = self
            .api
            .post_json_with_auth(
                &format!("/teams/{}/members", team.id),
                &admin.access_token,
                &AddMemberRequest {
                    nickname: user.nickname.clone(),
                },
            )
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("add member failed ({status}): {body}"));
        }

        Ok((user, team.id))
    }

    // ── HTTP convenience methods (delegate to ApiClient) ──────────────

    pub async fn get_authed(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.api.get_with_auth(path, token).await
    }

    pub async fn post_authed(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.api.post_with_auth(path, token).await
    }

    pub async fn post_json_authed<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        self.api.post_json_with_auth(path, token, body).await
    }

    pub async fn put_json_authed<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        self.api.put_json_with_auth(path, token, body).await
    }

    pub async fn delete_authed(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.api.delete_with_auth(path, token).await
    }
}
