//! Generic OAuth2 provider support.
//!
//! Config-driven: no provider-specific code branches. Any OAuth2 provider
//! (GitHub, GitLab, Gitea, OIDC-compatible) can be added via configuration.
//!
//! This module contains only types, URL builders, and JSON parsing.
//! No HTTP calls or DB access — those live in the backend adapters.

use serde::{Deserialize, Serialize};

use crate::ServiceError;

// ── Provider Configuration ──────────────────────────────────────────────────

/// OAuth2 provider configuration. Loaded from environment variables or config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProviderConfig {
    /// Unique provider identifier: "github", "gitlab-corp", "gitea-internal"
    pub id: String,
    /// UI display name: "GitHub", "GitLab (Corp)"
    pub display_name: String,

    // OAuth2 endpoints
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    /// Optional separate email endpoint (GitHub-specific: /user/emails)
    pub email_url: Option<String>,

    pub client_id: String,
    #[serde(skip_serializing)]
    pub client_secret: String,
    pub scopes: String,

    /// JSON field mapping from userinfo response to internal fields
    pub field_map: OAuthFieldMap,

    /// Skip TLS verification for self-hosted instances (dev only)
    #[serde(default)]
    pub tls_skip_verify: bool,

    /// External URL for browser redirects (may differ from token_url for Docker setups)
    pub external_authorize_url: Option<String>,
}

/// Maps provider-specific JSON field names to our internal fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthFieldMap {
    /// Field containing the user's unique ID: "id" (GitHub/GitLab) or "sub" (OIDC)
    pub id: String,
    /// Field containing the username: "login" (GitHub) or "username" (GitLab)
    pub username: String,
    /// Field containing the email: "email"
    pub email: String,
    /// Field containing the avatar URL: "avatar_url" or "picture"
    pub avatar: String,
}

/// Normalized user info extracted from any OAuth provider's userinfo response.
#[derive(Debug, Clone)]
pub struct OAuthUserInfo {
    /// Provider config id (e.g. "github")
    pub provider_id: String,
    /// Provider-side user ID (as string)
    pub provider_user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
}

// ── URL Builders (pure functions, no HTTP) ──────────────────────────────────

/// Build the OAuth authorize URL that the user's browser should be redirected to.
pub fn build_authorize_url(
    config: &OAuthProviderConfig,
    redirect_uri: &str,
    state: &str,
) -> String {
    let base = config
        .external_authorize_url
        .as_deref()
        .unwrap_or(&config.authorize_url);

    format!(
        "{}?client_id={}&redirect_uri={}&state={}&scope={}&response_type=code",
        base,
        urlencoding(&config.client_id),
        urlencoding(redirect_uri),
        urlencoding(state),
        urlencoding(&config.scopes),
    )
}

/// Build the JSON body for the token exchange request.
pub fn build_token_request_body(
    config: &OAuthProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> serde_json::Value {
    serde_json::json!({
        "client_id": config.client_id,
        "client_secret": config.client_secret,
        "code": code,
        "grant_type": "authorization_code",
        "redirect_uri": redirect_uri,
    })
}

/// Extract normalized user info from a provider's userinfo JSON response.
///
/// `email_json` is an optional array of email objects (GitHub `/user/emails` format)
/// used when the primary userinfo endpoint doesn't include the email.
pub fn extract_user_info(
    config: &OAuthProviderConfig,
    userinfo_json: &serde_json::Value,
    email_json: Option<&[serde_json::Value]>,
) -> Result<OAuthUserInfo, ServiceError> {
    // Extract provider user ID — may be number or string depending on provider
    let provider_user_id = match &userinfo_json[&config.field_map.id] {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => {
            return Err(ServiceError::Internal(format!(
                "OAuth userinfo missing '{}' field",
                config.field_map.id
            )))
        }
    };

    let username = userinfo_json[&config.field_map.username]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    // Email: try userinfo first, then email_json (GitHub format: [{email, primary, verified}])
    let email = userinfo_json[&config.field_map.email]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            email_json.and_then(|emails| {
                emails
                    .iter()
                    .find(|e| e["primary"].as_bool() == Some(true))
                    .and_then(|e| e["email"].as_str())
                    .map(|s| s.to_string())
            })
        });

    let avatar_url = userinfo_json[&config.field_map.avatar]
        .as_str()
        .map(|s| s.to_string());

    Ok(OAuthUserInfo {
        provider_id: config.id.clone(),
        provider_user_id,
        username,
        email,
        avatar_url,
    })
}

// ── Provider Presets ────────────────────────────────────────────────────────

/// Create a GitHub OAuth2 provider config. Only needs client credentials.
pub fn github_preset(client_id: String, client_secret: String) -> OAuthProviderConfig {
    OAuthProviderConfig {
        id: "github".into(),
        display_name: "GitHub".into(),
        authorize_url: "https://github.com/login/oauth/authorize".into(),
        token_url: "https://github.com/login/oauth/access_token".into(),
        userinfo_url: "https://api.github.com/user".into(),
        email_url: Some("https://api.github.com/user/emails".into()),
        client_id,
        client_secret,
        scopes: "read:user,user:email".into(),
        field_map: OAuthFieldMap {
            id: "id".into(),
            username: "login".into(),
            email: "email".into(),
            avatar: "avatar_url".into(),
        },
        tls_skip_verify: false,
        external_authorize_url: None,
    }
}

/// Create a GitLab OAuth2 provider config for a given instance URL.
///
/// `instance_url` is the server-accessible URL (e.g. `http://gitlab:80` in Docker).
/// `external_url` is the browser-accessible URL (e.g. `http://localhost:8929`).
/// If `external_url` is None, `instance_url` is used for browser redirects too.
pub fn gitlab_preset(
    instance_url: String,
    external_url: Option<String>,
    client_id: String,
    client_secret: String,
) -> OAuthProviderConfig {
    let base = instance_url.trim_end_matches('/');
    let ext_base = external_url
        .as_deref()
        .map(|u| u.trim_end_matches('/').to_string());

    OAuthProviderConfig {
        id: "gitlab".into(),
        display_name: "GitLab".into(),
        authorize_url: format!("{base}/oauth/authorize"),
        token_url: format!("{base}/oauth/token"),
        userinfo_url: format!("{base}/api/v4/user"),
        email_url: None, // GitLab includes email in /api/v4/user
        client_id,
        client_secret,
        scopes: "read_user".into(),
        field_map: OAuthFieldMap {
            id: "id".into(),
            username: "username".into(),
            email: "email".into(),
            avatar: "avatar_url".into(),
        },
        tls_skip_verify: false,
        external_authorize_url: ext_base.map(|b| format!("{b}/oauth/authorize")),
    }
}

// ── API Response Types ──────────────────────────────────────────────────────

/// Available auth providers (returned by GET /api/auth/providers).
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AuthProvidersResponse {
    pub email_password: bool,
    pub oauth: Vec<OAuthProviderInfo>,
}

/// Public info about an OAuth provider.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OAuthProviderInfo {
    pub id: String,
    pub display_name: String,
}

/// A linked OAuth provider shown in user settings.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct LinkedProvider {
    pub provider: String,
    pub provider_username: String,
    pub display_name: String,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn urlencoding(s: &str) -> String {
    // Minimal URL-encoding for OAuth parameters
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0f) as usize]));
            }
        }
    }
    out
}
