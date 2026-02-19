use anyhow::Result;
use serde::Serialize;

/// Holds connection info for a test run.
pub struct TestContext {
    base_url: String,
    client: reqwest::Client,
}

impl TestContext {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    /// Build a full API URL from a path like `/health`.
    pub fn url(&self, path: &str) -> String {
        format!("{}/api{}", self.base_url, path)
    }

    /// Unauthenticated GET returning the raw response.
    pub async fn get(&self, path: &str) -> Result<reqwest::Response> {
        Ok(self.client.get(self.url(path)).send().await?)
    }

    /// Unauthenticated POST with JSON body returning the raw response.
    pub async fn post_json<T: Serialize>(&self, path: &str, body: &T) -> Result<reqwest::Response> {
        Ok(self.client.post(self.url(path)).json(body).send().await?)
    }
}
