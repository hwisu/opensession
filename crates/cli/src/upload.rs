use anyhow::{bail, Context, Result};
use opensession_core::session::{
    build_git_storage_meta_json, is_auxiliary_session, working_directory,
};
use std::path::Path;
use std::time::Duration;

use crate::config::load_config;
use opensession_api_client::ApiClient;
use opensession_parsers::{all_parsers, parser_for_path};

/// Upload a session file to the configured server (or git branch with --git)
pub async fn run_upload(file: &Path, parent_ids: &[String], use_git: bool) -> Result<()> {
    if !file.exists() {
        bail!("File not found: {}", file.display());
    }

    let config = load_config()?;

    // Find a parser that can handle this file
    let parsers = all_parsers();
    let parser = parser_for_path(&parsers, file);

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
    if is_auxiliary_session(&session) {
        println!(
            "Skipping upload: auxiliary session '{}' is hidden by policy",
            session.session_id
        );
        return Ok(());
    }

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
    let repo_root = resolve_repo_root_for_session(session);

    let repo_root = match repo_root {
        Some(r) => r,
        None => bail!("Not inside a git repository. Run from a git repo or use server upload."),
    };

    println!(
        "Storing to git branch opensession/sessions in {}...",
        repo_root.display()
    );

    let hail_jsonl = session_to_hail_jsonl_bytes(session)?;
    let meta_json = build_git_storage_meta_json(session);

    let storage = opensession_git_native::NativeGitStorage;
    let rel_path = storage
        .store(&repo_root, &session.session_id, &hail_jsonl, &meta_json)
        .map_err(|e| anyhow::anyhow!("Git storage failed: {e}"))?;

    println!("Stored at: {} (branch: opensession/sessions)", rel_path);
    println!("Session ID: {}", session.session_id);

    Ok(())
}

fn resolve_repo_root_for_session(
    session: &opensession_core::Session,
) -> Option<std::path::PathBuf> {
    if let Some(cwd) = working_directory(session) {
        if let Some(repo_root) = opensession_git_native::ops::find_repo_root(Path::new(cwd)) {
            return Some(repo_root);
        }
    }

    std::env::current_dir()
        .ok()
        .and_then(|cwd| opensession_git_native::ops::find_repo_root(&cwd))
}

fn session_to_hail_jsonl_bytes(session: &opensession_core::Session) -> Result<Vec<u8>> {
    session
        .to_jsonl()
        .map(|jsonl| jsonl.into_bytes())
        .map_err(|e| anyhow::anyhow!("failed to serialize HAIL JSONL body: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{resolve_repo_root_for_session, session_to_hail_jsonl_bytes};
    use chrono::Utc;
    use opensession_core::session::ATTR_CWD;
    use opensession_core::{Agent, Content, Event, EventType, Session};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn cwd_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct CwdRestore {
        path: PathBuf,
    }

    impl CwdRestore {
        fn capture() -> Self {
            Self {
                path: std::env::current_dir().expect("capture current directory"),
            }
        }
    }

    impl Drop for CwdRestore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.path);
        }
    }

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

    #[test]
    fn resolve_repo_root_prefers_session_working_directory() {
        let _lock = cwd_test_lock().lock().expect("lock cwd test");
        let _restore = CwdRestore::capture();
        let root = tempdir().expect("temp dir");

        let current_repo = root.path().join("current-repo");
        let session_repo = root.path().join("session-repo");
        std::fs::create_dir_all(current_repo.join(".git")).expect("create current .git");
        std::fs::create_dir_all(session_repo.join(".git")).expect("create session .git");

        let current_nested = current_repo.join("nested/path");
        let session_nested = session_repo.join("nested/path");
        std::fs::create_dir_all(&current_nested).expect("create current nested");
        std::fs::create_dir_all(&session_nested).expect("create session nested");

        std::env::set_current_dir(&current_nested).expect("set current dir");

        let mut session = Session::new(
            "s-cli-repo".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.context.attributes.insert(
            ATTR_CWD.to_string(),
            serde_json::Value::String(session_nested.to_string_lossy().to_string()),
        );

        let resolved = resolve_repo_root_for_session(&session).expect("resolve repo root");
        assert_eq!(resolved, session_repo);
    }
}
