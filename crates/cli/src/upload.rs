use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Duration;

use crate::config::load_config;
use opensession_api_client::ApiClient;
use opensession_parsers::{all_parsers, SessionParser};

/// Upload a session file to the configured server (or git branch with --git)
pub async fn run_upload(file: &Path, parent_ids: &[String], use_git: bool) -> Result<()> {
    if !file.exists() {
        bail!("File not found: {}", file.display());
    }

    let config = load_config()?;

    // Find a parser that can handle this file
    let parsers = all_parsers();
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(file))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => bail!(
            "No parser found for file: {}\nSupported formats: Claude Code (.jsonl), Codex (.jsonl), OpenCode (.json), Cline, Amp, Cursor, Gemini",
            file.display()
        ),
    };

    println!("Parsing with {} parser...", parser.name());
    let session = parser
        .parse(file)
        .with_context(|| format!("Failed to parse {}", file.display()))?;

    // Check exclude_tools
    if config
        .privacy
        .exclude_tools
        .iter()
        .any(|t| t.eq_ignore_ascii_case(&session.agent.tool))
    {
        println!(
            "Skipping upload: tool '{}' is in exclude_tools list",
            session.agent.tool
        );
        return Ok(());
    }

    println!(
        "Parsed session: {} ({} events, {} tool calls)",
        session.session_id, session.stats.event_count, session.stats.tool_call_count
    );

    if use_git {
        return upload_to_git(&session);
    }

    // Server upload path (git-native default: local scope without auth/team setup).
    let target_team_id = if config.server.team_id.trim().is_empty() {
        "local".to_string()
    } else {
        config.server.team_id.clone()
    };
    println!("Uploading to {}...", config.server.url);
    println!("Target scope: {}", target_team_id);

    let mut client = ApiClient::new(&config.server.url, Duration::from_secs(60))?;
    if !config.server.api_key.trim().is_empty() {
        client.set_auth(config.server.api_key.clone());
    }

    let linked = if parent_ids.is_empty() {
        None
    } else {
        Some(parent_ids.to_vec())
    };

    let resp = client
        .upload_session(&opensession_api_client::opensession_api::UploadRequest {
            session,
            team_id: Some(target_team_id),
            body_url: None,
            linked_session_ids: linked,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            score_plugin: None,
        })
        .await?;

    println!("Upload successful!");
    println!("Session ID: {}", resp.id);

    Ok(())
}

/// Store session to the git branch in the current repo.
fn upload_to_git(session: &opensession_core::Session) -> Result<()> {
    // Determine repo root from session cwd or current dir
    let cwd = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let repo_root = std::env::current_dir()
        .ok()
        .and_then(|d| {
            // Walk up from cwd to find .git
            let mut dir = d;
            loop {
                if dir.join(".git").exists() {
                    return Some(dir);
                }
                if !dir.pop() {
                    return None;
                }
            }
        })
        .or_else(|| {
            // Fallback to session's working directory
            let mut dir = std::path::PathBuf::from(cwd);
            loop {
                if dir.join(".git").exists() {
                    return Some(dir);
                }
                if !dir.pop() {
                    return None;
                }
            }
        });

    let repo_root = match repo_root {
        Some(r) => r,
        None => bail!("Not inside a git repository. Run from a git repo or use server upload."),
    };

    println!(
        "Storing to git branch opensession/sessions in {}...",
        repo_root.display()
    );

    let hail_jsonl = serde_json::to_vec(session)?;
    let meta_json = serde_json::to_string_pretty(&serde_json::json!({
        "session_id": session.session_id,
        "title": session.context.title,
        "tool": session.agent.tool,
        "model": session.agent.model,
        "stats": session.stats,
    }))?
    .into_bytes();

    let storage = opensession_git_native::NativeGitStorage;
    let rel_path = storage
        .store(&repo_root, &session.session_id, &hail_jsonl, &meta_json)
        .map_err(|e| anyhow::anyhow!("Git storage failed: {e}"))?;

    println!("Stored at: {} (branch: opensession/sessions)", rel_path);
    println!("Session ID: {}", session.session_id);

    Ok(())
}
