use crate::oauth;
use serde::{Deserialize, Serialize};

/// Email + password registration.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthRegisterRequest {
    pub email: String,
    pub password: String,
    pub nickname: String,
}

/// Email + password login.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Returned on successful login / register / refresh.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub user_id: String,
    pub nickname: String,
}

/// Refresh token request.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Logout request (invalidate refresh token).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// Change password request.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Returned by `POST /api/auth/verify` — confirms token validity.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct VerifyResponse {
    pub user_id: String,
    pub nickname: String,
}

/// Full user profile returned by `GET /api/auth/me`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct UserSettingsResponse {
    pub user_id: String,
    pub nickname: String,
    pub created_at: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub oauth_providers: Vec<oauth::LinkedProvider>,
}

/// Generic success response for operations that don't return data.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OkResponse {
    pub ok: bool,
}

/// Response for API key issuance. The key is visible only at issuance time.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct IssueApiKeyResponse {
    pub api_key: String,
}

/// Public metadata for a user-managed git credential.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct GitCredentialSummary {
    pub id: String,
    pub label: String,
    pub host: String,
    pub path_prefix: String,
    pub header_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

/// Response for `GET /api/auth/git-credentials`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ListGitCredentialsResponse {
    #[serde(default)]
    pub credentials: Vec<GitCredentialSummary>,
}

/// Request for `POST /api/auth/git-credentials`.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateGitCredentialRequest {
    pub label: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    pub header_name: String,
    pub header_value: String,
}

/// Response for OAuth link initiation (redirect URL).
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OAuthLinkResponse {
    pub url: String,
}
