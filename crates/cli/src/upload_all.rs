use anyhow::{bail, Result};
use opensession_core::session::is_auxiliary_session;
use std::time::Duration;

use crate::config::load_config;
use opensession_api_client::retry::{retry_post, RetryConfig};
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{is_auxiliary_session_path, parse_with_default_parsers};

/// Discover all local sessions and upload them to the server.
pub async fn run_upload_all() -> Result<()> {
    let config = load_config()?;
    if config.server.api_key.trim().is_empty() {
        bail!("API key not configured. Run: opensession account connect --api-key <key>");
    }

    let locations = discover_sessions();
    if locations.is_empty() {
        println!("No AI sessions found on this machine.");
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let url = format!("{}/api/sessions", config.server.url.trim_end_matches('/'));
    let retry_cfg = RetryConfig::default();

    let mut total = 0usize;
    let mut success = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for loc in &locations {
        println!("\n[{}] {} file(s)", loc.tool, loc.paths.len());

        for path in &loc.paths {
            total += 1;

            // Skip subagent files
            if is_auxiliary_session_path(path) {
                skipped += 1;
                continue;
            }

            // Parse
            let session = match parse_with_default_parsers(path) {
                Ok(Some(session)) => session,
                Ok(None) => {
                    skipped += 1;
                    continue;
                }
                Err(e) => {
                    eprintln!("  FAIL parse {}: {}", path.display(), e);
                    failed += 1;
                    continue;
                }
            };
            if is_auxiliary_session(&session) {
                skipped += 1;
                continue;
            }

            // Skip empty sessions (no events)
            if session.events.is_empty() {
                skipped += 1;
                continue;
            }

            // Check exclude_tools
            if config
                .privacy
                .exclude_tools
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&session.agent.tool))
            {
                skipped += 1;
                continue;
            }

            // Upload with retry
            let upload_body = serde_json::json!({
                "session": session,
            });

            match retry_post(
                &client,
                &url,
                Some(&config.server.api_key),
                &upload_body,
                &retry_cfg,
            )
            .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let title = session.context.title.as_deref().unwrap_or("(untitled)");
                    println!(
                        "  OK  {} ({} events) - {}",
                        title,
                        session.stats.event_count,
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    success += 1;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    eprintln!(
                        "  FAIL upload {} (HTTP {}): {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        status,
                        body
                    );
                    failed += 1;
                }
                Err(e) => {
                    eprintln!(
                        "  FAIL upload {}: {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        e
                    );
                    failed += 1;
                }
            }
        }
    }

    println!("\n--- Summary ---");
    println!("Total:   {}", total);
    println!("Success: {}", success);
    println!("Skipped: {} (subagent/empty/no parser)", skipped);
    println!("Failed:  {}", failed);

    Ok(())
}
