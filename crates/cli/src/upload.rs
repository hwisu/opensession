use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::config::load_config;
use opensession_parsers::{all_parsers, SessionParser};

/// Upload a session file to the configured server
pub async fn run_upload(file: &Path) -> Result<()> {
    if !file.exists() {
        bail!("File not found: {}", file.display());
    }

    let config = load_config()?;
    if config.server.api_key.is_empty() {
        bail!(
            "API key not configured. Run: opensession config --api-key <key>"
        );
    }

    // Find a parser that can handle this file
    let parsers = all_parsers();
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(file))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => bail!(
            "No parser found for file: {}\nSupported formats: Claude Code (.jsonl), OpenCode (.json), Goose (.db), Aider, Cursor",
            file.display()
        ),
    };

    println!("Parsing with {} parser...", parser.name());
    let session = parser
        .parse(file)
        .with_context(|| format!("Failed to parse {}", file.display()))?;

    println!(
        "Parsed session: {} ({} events, {} tool calls)",
        session.session_id, session.stats.event_count, session.stats.tool_call_count
    );

    // Upload to server
    let url = format!("{}/api/sessions", config.server.url.trim_end_matches('/'));
    println!("Uploading to {}...", url);

    let upload_body = serde_json::json!({
        "session": session,
        "visibility": "public"
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.server.api_key))
        .header("Content-Type", "application/json")
        .json(&upload_body)
        .send()
        .await
        .context("Failed to connect to server")?;

    let status = response.status();
    if status.is_success() {
        let body: serde_json::Value = response
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({"status": "ok"}));
        println!("Upload successful!");
        if let Some(id) = body.get("id") {
            println!("Session ID: {}", id);
        }
    } else {
        let body = response.text().await.unwrap_or_default();
        bail!("Upload failed (HTTP {}): {}", status, body);
    }

    Ok(())
}
