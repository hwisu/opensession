use std::time::Duration;

use anyhow::{bail, Result};
use serde::Serialize;

use opensession_api::*;

/// Typed HTTP client for the OpenSession API.
///
/// Provides high-level methods for each API endpoint (using the stored auth
/// token) and low-level `*_with_auth` methods for callers that need per-request
/// auth (e.g. E2E tests exercising multiple users).
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
}

impl ApiClient {
    /// Create a new client with the given base URL and timeout.
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self> {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token: None,
        })
    }

    /// Create from an existing `reqwest::Client` (e.g. shared in tests).
    pub fn with_client(client: reqwest::Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_token: None,
        }
    }

    pub fn set_auth(&mut self, token: String) {
        self.auth_token = Some(token);
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Access the underlying `reqwest::Client`.
    pub fn reqwest_client(&self) -> &reqwest::Client {
        &self.client
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api{}", self.base_url, path)
    }

    fn token_or_bail(&self) -> Result<&str> {
        self.auth_token
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("auth token not set"))
    }

    // ── Health ────────────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self.client.get(self.url("/health")).send().await?;
        parse_response(resp).await
    }

    // ── Auth ──────────────────────────────────────────────────────────────

    pub async fn login(&self, req: &LoginRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/login"))
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn register(&self, req: &AuthRegisterRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/register"))
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn verify(&self) -> Result<VerifyResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .post(self.url("/auth/verify"))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn me(&self) -> Result<UserSettingsResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .get(self.url("/auth/me"))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn refresh(&self, req: &RefreshRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/refresh"))
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn logout(&self, req: &LogoutRequest) -> Result<OkResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .post(self.url("/auth/logout"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn change_password(&self, req: &ChangePasswordRequest) -> Result<OkResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .post(self.url("/auth/change-password"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn regenerate_key(&self) -> Result<RegenerateKeyResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .post(self.url("/auth/regenerate-key"))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    // ── Sessions ──────────────────────────────────────────────────────────

    pub async fn upload_session(&self, req: &UploadRequest) -> Result<UploadResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .post(self.url("/sessions"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn list_sessions(&self, query: &SessionListQuery) -> Result<SessionListResponse> {
        let token = self.token_or_bail()?;
        let mut url = self.url("/sessions");

        // Build query string from the struct fields
        let mut params = Vec::new();
        params.push(format!("page={}", query.page));
        params.push(format!("per_page={}", query.per_page));
        if let Some(ref s) = query.search {
            params.push(format!("search={s}"));
        }
        if let Some(ref t) = query.tool {
            params.push(format!("tool={t}"));
        }
        if let Some(ref s) = query.sort {
            params.push(format!("sort={s}"));
        }
        if let Some(ref r) = query.time_range {
            params.push(format!("time_range={r}"));
        }
        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }

        let resp = self.client.get(&url).bearer_auth(token).send().await?;
        parse_response(resp).await
    }

    pub async fn get_session(&self, id: &str) -> Result<SessionDetail> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .get(self.url(&format!("/sessions/{id}")))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn delete_session(&self, id: &str) -> Result<OkResponse> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .delete(self.url(&format!("/sessions/{id}")))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    pub async fn get_session_raw(&self, id: &str) -> Result<serde_json::Value> {
        let token = self.token_or_bail()?;
        let resp = self
            .client
            .get(self.url(&format!("/sessions/{id}/raw")))
            .bearer_auth(token)
            .send()
            .await?;
        parse_response(resp).await
    }

    // ── Raw helpers (for E2E / advanced usage) ────────────────────────────

    /// Authenticated GET returning the raw response.
    pub async fn get_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        Ok(self
            .client
            .get(self.url(path))
            .bearer_auth(token)
            .send()
            .await?)
    }

    /// Authenticated POST (no body) returning the raw response.
    pub async fn post_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        Ok(self
            .client
            .post(self.url(path))
            .bearer_auth(token)
            .send()
            .await?)
    }

    /// Authenticated POST with JSON body returning the raw response.
    pub async fn post_json_with_auth<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        Ok(self
            .client
            .post(self.url(path))
            .bearer_auth(token)
            .json(body)
            .send()
            .await?)
    }

    /// Authenticated PUT with JSON body returning the raw response.
    pub async fn put_json_with_auth<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        Ok(self
            .client
            .put(self.url(path))
            .bearer_auth(token)
            .json(body)
            .send()
            .await?)
    }

    /// Authenticated DELETE returning the raw response.
    pub async fn delete_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        Ok(self
            .client
            .delete(self.url(path))
            .bearer_auth(token)
            .send()
            .await?)
    }

    /// Unauthenticated POST with JSON body returning the raw response.
    pub async fn post_json_raw<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        Ok(self.client.post(self.url(path)).json(body).send().await?)
    }
}

/// Parse an HTTP response: return the deserialized body on 2xx,
/// or an error containing the status and body text.
async fn parse_response<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("{status}: {body}");
    }
    Ok(resp.json().await?)
}
