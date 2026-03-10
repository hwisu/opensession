use std::time::Duration;

use serde::Serialize;
use thiserror::Error;

use opensession_api::*;

pub type Result<T> = std::result::Result<T, ApiClientError>;

#[derive(Debug, Error)]
pub enum ApiClientError {
    #[error("auth token not set")]
    AuthTokenMissing,
    #[error("transport error: {0}")]
    Transport(reqwest::Error),
    #[error("unexpected API status {status}: {body}")]
    UnexpectedStatus {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("response decode error: {0}")]
    Decode(reqwest::Error),
}

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
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(ApiClientError::Transport)?;
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
        let normalized = token.trim();
        if normalized.is_empty() {
            self.auth_token = None;
            return;
        }
        self.auth_token = Some(normalized.to_string());
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

    fn token_or_err(&self) -> Result<&str> {
        self.auth_token
            .as_deref()
            .ok_or(ApiClientError::AuthTokenMissing)
    }

    // ── Health ────────────────────────────────────────────────────────────

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .client
            .get(self.url("/health"))
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    // ── Auth ──────────────────────────────────────────────────────────────

    pub async fn login(&self, req: &LoginRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/login"))
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn register(&self, req: &AuthRegisterRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/register"))
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn verify(&self) -> Result<VerifyResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .post(self.url("/auth/verify"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn me(&self) -> Result<UserSettingsResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .get(self.url("/auth/me"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn refresh(&self, req: &RefreshRequest) -> Result<AuthTokenResponse> {
        let resp = self
            .client
            .post(self.url("/auth/refresh"))
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn logout(&self, req: &LogoutRequest) -> Result<OkResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .post(self.url("/auth/logout"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn change_password(&self, req: &ChangePasswordRequest) -> Result<OkResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .post(self.url("/auth/change-password"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn issue_api_key(&self) -> Result<IssueApiKeyResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .post(self.url("/auth/api-keys/issue"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    // ── Sessions ──────────────────────────────────────────────────────────

    pub async fn upload_session(&self, req: &UploadRequest) -> Result<UploadResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .post(self.url("/sessions"))
            .bearer_auth(token)
            .json(req)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn list_sessions(&self, query: &SessionListQuery) -> Result<SessionListResponse> {
        let token = self.token_or_err()?;
        let mut url = self.url("/sessions");

        let mut params = Vec::new();
        params.push(format!("page={}", query.page));
        params.push(format!("per_page={}", query.per_page));
        if let Some(ref s) = query.search {
            params.push(format!("search={s}"));
        }
        if let Some(ref t) = query.tool {
            params.push(format!("tool={t}"));
        }
        if let Some(protocol) = query.protocol {
            params.push(format!("protocol={protocol}"));
        }
        if let Some(ref job_id) = query.job_id {
            params.push(format!("job_id={job_id}"));
        }
        if let Some(ref run_id) = query.run_id {
            params.push(format!("run_id={run_id}"));
        }
        if let Some(stage) = query.stage {
            params.push(format!("stage={stage}"));
        }
        if let Some(review_kind) = query.review_kind {
            params.push(format!("review_kind={review_kind}"));
        }
        if let Some(status) = query.status {
            params.push(format!("status={status}"));
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

        let resp = self
            .client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn get_session(&self, id: &str) -> Result<SessionDetail> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .get(self.url(&format!("/sessions/{id}")))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn delete_session(&self, id: &str) -> Result<OkResponse> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .delete(self.url(&format!("/sessions/{id}")))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    pub async fn get_session_raw(&self, id: &str) -> Result<serde_json::Value> {
        let token = self.token_or_err()?;
        let resp = self
            .client
            .get(self.url(&format!("/sessions/{id}/raw")))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)?;
        parse_response(resp).await
    }

    // ── Raw helpers (for E2E / advanced usage) ────────────────────────────

    /// Authenticated GET returning the raw response.
    pub async fn get_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.client
            .get(self.url(path))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }

    /// Authenticated POST (no body) returning the raw response.
    pub async fn post_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.client
            .post(self.url(path))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }

    /// Authenticated POST with JSON body returning the raw response.
    pub async fn post_json_with_auth<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        self.client
            .post(self.url(path))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }

    /// Authenticated PUT with JSON body returning the raw response.
    pub async fn put_json_with_auth<T: Serialize>(
        &self,
        path: &str,
        token: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        self.client
            .put(self.url(path))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }

    /// Authenticated DELETE returning the raw response.
    pub async fn delete_with_auth(&self, path: &str, token: &str) -> Result<reqwest::Response> {
        self.client
            .delete(self.url(path))
            .bearer_auth(token)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }

    /// Unauthenticated POST with JSON body returning the raw response.
    pub async fn post_json_raw<T: Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        self.client
            .post(self.url(path))
            .json(body)
            .send()
            .await
            .map_err(ApiClientError::Transport)
    }
}

async fn parse_response<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    if !status.is_success() {
        let body = match resp.text().await {
            Ok(body) => body,
            Err(err) => format!("<failed to read response body: {err}>"),
        };
        return Err(ApiClientError::UnexpectedStatus { status, body });
    }
    resp.json().await.map_err(ApiClientError::Decode)
}

#[cfg(test)]
mod tests {
    use super::{ApiClient, ApiClientError};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener address");

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });

        format!("http://{addr}")
    }

    #[test]
    fn set_auth_trims_surrounding_whitespace() {
        let mut client = ApiClient::new("https://example.com", Duration::from_secs(1))
            .expect("client should construct");

        client.set_auth("  osk_test_token  ".to_string());
        assert_eq!(client.auth_token(), Some("osk_test_token"));
    }

    #[test]
    fn set_auth_clears_auth_for_blank_tokens() {
        let mut client = ApiClient::new("https://example.com", Duration::from_secs(1))
            .expect("client should construct");

        client.set_auth("osk_test_token".to_string());
        assert_eq!(client.auth_token(), Some("osk_test_token"));

        client.set_auth("   ".to_string());
        assert_eq!(client.auth_token(), None);
    }

    #[tokio::test]
    async fn verify_without_auth_token_returns_typed_error() {
        let client = ApiClient::new("https://example.com", Duration::from_secs(1))
            .expect("client should construct");

        let error = client.verify().await.expect_err("verify should fail");
        assert!(matches!(error, ApiClientError::AuthTokenMissing));
    }

    #[tokio::test]
    async fn parse_response_surfaces_unexpected_status_with_body() {
        let base_url = serve_once(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nmissing auth",
        )
        .await;
        let client =
            ApiClient::new(&base_url, Duration::from_secs(1)).expect("client should construct");

        let error = client.health().await.expect_err("health should fail");
        match error {
            ApiClientError::UnexpectedStatus { status, body } => {
                assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED);
                assert_eq!(body, "missing auth");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn parse_response_surfaces_decode_errors() {
        let base_url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nContent-Type: application/json\r\nConnection: close\r\n\r\nnot-json",
        )
        .await;
        let client =
            ApiClient::new(&base_url, Duration::from_secs(1)).expect("client should construct");

        let error = client.health().await.expect_err("health should fail");
        assert!(matches!(error, ApiClientError::Decode(_)));
    }

    #[tokio::test]
    async fn invalid_base_url_surfaces_transport_error() {
        let client =
            ApiClient::new("not-a-url", Duration::from_secs(1)).expect("client should construct");

        let error = client.health().await.expect_err("health should fail");
        assert!(matches!(error, ApiClientError::Transport(_)));
    }
}
