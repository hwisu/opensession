use anyhow::{bail, Result};
use std::time::Duration;

use crate::config::load_config;
use crate::server::retry_upload;
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};

/// Discover all local sessions and upload them to the server.
pub async fn run_upload_all() -> Result<()> {
    let config = load_config()?;
    if config.server.api_key.is_empty() {
        bail!("API key not configured. Run: opensession config --api-key <key>");
    }
    if config.server.team_id.is_empty() {
        bail!("Team ID not configured. Run: opensession config --team-id <id>");
    }

    let locations = discover_sessions();
    if locations.is_empty() {
        println!("No AI sessions found on this machine.");
        return Ok(());
    }

    let parsers = all_parsers();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let url = format!("{}/api/sessions", config.server.url.trim_end_matches('/'));

    let mut total = 0usize;
    let mut success = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for loc in &locations {
        println!("\n[{}] {} file(s)", loc.tool, loc.paths.len());

        for path in &loc.paths {
            total += 1;

            // Skip subagent files
            let path_str = path.to_string_lossy();
            if path_str.contains("/subagents/")
                || path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("agent-"))
            {
                skipped += 1;
                continue;
            }

            // Find parser
            let parser: Option<&dyn SessionParser> = parsers
                .iter()
                .find(|p| p.can_parse(path))
                .map(|p| p.as_ref());

            let parser = match parser {
                Some(p) => p,
                None => {
                    skipped += 1;
                    continue;
                }
            };

            // Parse
            let session = match parser.parse(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  FAIL parse {}: {}", path.display(), e);
                    failed += 1;
                    continue;
                }
            };

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
                "team_id": config.server.team_id,
            });

            match retry_upload(&client, &url, &config.server.api_key, &upload_body).await {
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
