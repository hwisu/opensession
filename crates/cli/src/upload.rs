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

    println!("Uploading to {}...", config.server.url);

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

    let hail_jsonl = session_to_hail_jsonl_bytes(session)?;
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

fn session_to_hail_jsonl_bytes(session: &opensession_core::Session) -> Result<Vec<u8>> {
    session
        .to_jsonl()
        .map(|jsonl| jsonl.into_bytes())
        .map_err(|e| anyhow::anyhow!("failed to serialize HAIL JSONL body: {e}"))
}

#[cfg(test)]
mod tests {
    use super::session_to_hail_jsonl_bytes;
    use chrono::Utc;
    use opensession_core::{Agent, Content, Event, EventType, Session};

    #[test]
    fn git_upload_body_uses_hail_jsonl_lines() {
        let mut session = Session::new(
            "s-cli-upload".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.events.push(Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("hello"),
            duration_ms: None,
            attributes: Default::default(),
        });
        session.recompute_stats();

        let body = session_to_hail_jsonl_bytes(&session).expect("serialize session as HAIL JSONL");
        let text = String::from_utf8(body).expect("HAIL JSONL body should be UTF-8");
        let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 3, "expected header/event/stats JSONL lines");

        let header: serde_json::Value = serde_json::from_str(lines[0]).expect("valid header JSON");
        assert_eq!(header["type"], "header");
        let event: serde_json::Value = serde_json::from_str(lines[1]).expect("valid event JSON");
        assert_eq!(event["type"], "event");
        let stats: serde_json::Value = serde_json::from_str(lines[2]).expect("valid stats JSON");
        assert_eq!(stats["type"], "stats");
    }
}
