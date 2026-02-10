use anyhow::Result;
use opensession_core::trace::{Agent, Event, SessionContext};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Client for streaming events to the server incrementally.
pub struct StreamClient {
    client: reqwest::Client,
    server_url: String,
    api_key: String,
    team_id: String,
    /// Session ID on the server (assigned after first batch creates the session)
    remote_session_id: Option<String>,
}

impl StreamClient {
    pub fn new(server_url: String, api_key: String, team_id: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            client,
            server_url,
            api_key,
            team_id,
            remote_session_id: None,
        }
    }

    /// Send a batch of events. On the first call, includes agent/context to create the session.
    pub async fn send_events(
        &mut self,
        session_id: &str,
        agent: Option<&Agent>,
        context: Option<&SessionContext>,
        events: &[Event],
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let url = if let Some(ref remote_id) = self.remote_session_id {
            format!(
                "{}/api/sessions/{}/events",
                self.server_url.trim_end_matches('/'),
                remote_id
            )
        } else {
            format!(
                "{}/api/sessions/{}/events",
                self.server_url.trim_end_matches('/'),
                session_id
            )
        };

        let mut body = serde_json::json!({
            "events": events,
        });

        // Include agent/context on first batch to create session
        if self.remote_session_id.is_none() {
            if let Some(agent) = agent {
                body["agent"] = serde_json::to_value(agent)?;
            }
            if let Some(context) = context {
                body["context"] = serde_json::to_value(context)?;
            }
            body["team_id"] = serde_json::Value::String(self.team_id.clone());
        }

        let mut req = self.client.post(&url);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        match req.json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                debug!("Streamed {} events to {}", events.len(), session_id);

                // Try to extract session ID from response
                if self.remote_session_id.is_none() {
                    if let Ok(resp_body) = resp.json::<serde_json::Value>().await {
                        if let Some(id) = resp_body.get("id").and_then(|v| v.as_str()) {
                            self.remote_session_id = Some(id.to_string());
                            info!("Streaming session created: {}", id);
                        }
                    }
                }

                Ok(())
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status.as_u16() == 404 {
                    warn!(
                        "Streaming endpoint not available (404). \
                         Server may not support streaming yet."
                    );
                } else {
                    error!("Stream upload failed (HTTP {}): {}", status, body);
                }
                Ok(())
            }
            Err(e) => {
                warn!("Stream upload error: {}", e);
                Ok(())
            }
        }
    }

    /// Check if this client has an established remote session
    pub fn has_remote_session(&self) -> bool {
        self.remote_session_id.is_some()
    }

    /// Reset the client state (e.g., when a new session file is detected)
    pub fn reset(&mut self) {
        self.remote_session_id = None;
    }
}
