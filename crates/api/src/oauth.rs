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

/// Build OAuth2 token request as application/x-www-form-urlencoded pairs.
///
/// OAuth2 token exchange endpoints are required to support urlencoded form input.
pub fn build_token_request_form(
    config: &OAuthProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> Vec<(String, String)> {
    vec![
        ("client_id".into(), config.client_id.clone()),
        ("client_secret".into(), config.client_secret.clone()),
        ("code".into(), code.to_string()),
        ("grant_type".into(), "authorization_code".into()),
        ("redirect_uri".into(), redirect_uri.to_string()),
    ]
}

/// Build OAuth2 token request as x-www-form-urlencoded string.
pub fn build_token_request_form_encoded(
    config: &OAuthProviderConfig,
    code: &str,
    redirect_uri: &str,
) -> String {
    build_token_request_form(config, code, redirect_uri)
        .into_iter()
        .map(|(k, v)| format!("{}={}", urlencoding(&k), urlencoding(&v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Parse access_token from OAuth token response.
///
/// Supports both JSON (`{\"access_token\":\"...\"}`) and query-string style
/// (`access_token=...&scope=...`) payloads.
pub fn parse_access_token_response(raw: &str) -> Result<String, ServiceError> {
    let body = raw.trim();
    if body.is_empty() {
        return Err(ServiceError::Internal(
            "OAuth token exchange failed: empty response body".into(),
        ));
    }

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(token) = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(token.to_string());
        }

        let err = json.get("error").and_then(|v| v.as_str());
        let err_desc = json
            .get("error_description")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("error_message").and_then(|v| v.as_str()));

        let detail = match (err, err_desc) {
            (Some(e), Some(d)) if !d.is_empty() => format!("{e}: {d}"),
            (Some(e), _) => e.to_string(),
            (_, Some(d)) if !d.is_empty() => d.to_string(),
            _ => "no access_token field in JSON response".to_string(),
        };

        return Err(ServiceError::Internal(format!(
            "OAuth token exchange failed: {detail}"
        )));
    }

    let mut access_token: Option<String> = None;
    let mut error: Option<String> = None;
    let mut error_description: Option<String> = None;

    for pair in body.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_form_component(k);
        let value = decode_form_component(v);
        match key.as_str() {
            "access_token" if !value.trim().is_empty() => access_token = Some(value),
            "error" if !value.trim().is_empty() => error = Some(value),
            "error_description" if !value.trim().is_empty() => error_description = Some(value),
            _ => {}
        }
    }

    if let Some(token) = access_token {
        return Ok(token);
    }

    let detail = match (error, error_description) {
        (Some(e), Some(d)) => format!("{e}: {d}"),
        (Some(e), None) => e,
        (None, Some(d)) => d,
        (None, None) => "no access_token field in response".to_string(),
    };

    Err(ServiceError::Internal(format!(
        "OAuth token exchange failed: {detail}"
    )))
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

fn decode_form_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_value(bytes[i + 1]);
                let lo = hex_value(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        b'A'..=b'F' => Some(10 + b - b'A'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{github_preset, parse_access_token_response};

    #[test]
    fn parse_access_token_json_ok() {
        let raw = r#"{"access_token":"gho_123","scope":"read:user","token_type":"bearer"}"#;
        let token = parse_access_token_response(raw).expect("token parse");
        assert_eq!(token, "gho_123");
    }

    #[test]
    fn parse_access_token_form_ok() {
        let raw = "access_token=gho_abc&scope=read%3Auser&token_type=bearer";
        let token = parse_access_token_response(raw).expect("token parse");
        assert_eq!(token, "gho_abc");
    }

    #[test]
    fn parse_access_token_json_error_has_reason() {
        let raw = r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#;
        let err = parse_access_token_response(raw).expect_err("must fail");
        assert!(err.message().contains("bad_verification_code"));
    }

    #[test]
    fn build_form_encoded_contains_required_fields() {
        let provider = github_preset("cid".into(), "secret".into());
        let encoded =
            super::build_token_request_form_encoded(&provider, "code-1", "https://app/callback");
        assert!(encoded.contains("client_id=cid"));
        assert!(encoded.contains("client_secret=secret"));
        assert!(encoded.contains("grant_type=authorization_code"));
        assert!(encoded.contains("code=code-1"));
    }
}
