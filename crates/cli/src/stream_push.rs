//! `opensession daemon stream-push --agent <agent>` — incremental local session indexing.
//!
//! Called by the agent's PostToolUse hook on every tool use. Must be fast
//! (< 2s). Parses the full session file and upserts it into the local DB.
//! The daemon handles uploading to the server via debounced file watching.

use anyhow::{bail, Context, Result};
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use opensession_parsers::is_auxiliary_session_path;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

// ── Stream state persistence ─────────────────────────────────────────────

/// State persisted between stream-push invocations for a given session file.
#[derive(Debug, Serialize, Deserialize, Default)]
struct StreamState {
    file_path: String,
    byte_offset: u64,
}

fn state_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join("stream-state"))
}

fn file_hash(path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn state_file_path(session_file: &str) -> Result<PathBuf> {
    let dir = state_dir()?;
    Ok(dir.join(format!("{}.json", file_hash(session_file))))
}

fn load_state(path: &Path) -> Option<StreamState> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn save_state(path: &Path, state: &StreamState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

// ── Session file discovery ───────────────────────────────────────────────

/// Resolve the active session file for an agent based on the current working directory.
fn resolve_session_file(agent: &str) -> Result<PathBuf> {
    match agent {
        "claude-code" => resolve_claude_code_session(),
        _ => bail!("Unsupported agent for stream-push: {agent}"),
    }
}

/// Find the most recently modified Claude Code JSONL for the current project.
///
/// Claude Code stores sessions under `~/.claude/projects/<project-dir>/`.
/// The project directory name is the CWD with `/` replaced by `-`.
fn resolve_claude_code_session() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;

    let cwd = std::env::current_dir().context("Could not determine current directory")?;
    let cwd_str = cwd.to_string_lossy();

    // Claude Code project dir: CWD with '/' replaced by '-'
    let project_dir_name = cwd_str.replace('/', "-");
    let projects_dir = PathBuf::from(&home).join(".claude").join("projects");
    let project_dir = projects_dir.join(&project_dir_name);

    if !project_dir.is_dir() {
        bail!(
            "No Claude Code project directory found at {}",
            project_dir.display()
        );
    }

    // Find the most recently modified JSONL file (excluding subagents)
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;

    for entry in std::fs::read_dir(&project_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        // Skip subagent files
        if is_auxiliary_session_path(&path) {
            continue;
        }
        if let Ok(meta) = path.metadata() {
            if let Ok(modified) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| modified > *t) {
                    best = Some((path, modified));
                }
            }
        }
    }

    best.map(|(p, _)| p)
        .ok_or_else(|| anyhow::anyhow!("No active session file found in {}", project_dir.display()))
}

// ── Main command ─────────────────────────────────────────────────────────

/// Run the `stream-push` command: parse the session file and upsert to local DB.
pub fn run_stream_push(agent: &str) -> Result<()> {
    let session_file = resolve_session_file(agent)?;
    let file_path_str = session_file.to_string_lossy().to_string();

    // Load or create state
    let sp = state_file_path(&file_path_str)?;
    let mut state = load_state(&sp).unwrap_or_else(|| StreamState {
        file_path: file_path_str.clone(),
        ..Default::default()
    });

    // Check if file has changed (byte offset comparison)
    let file_len = std::fs::metadata(&session_file)?.len();
    if file_len == state.byte_offset {
        return Ok(());
    }

    // Parse the full file with the standard parser.
    let session = opensession_parsers::parse_with_default_parsers(&session_file)?
        .ok_or_else(|| anyhow::anyhow!("No parser for {}", session_file.display()))?;
    if is_auxiliary_session(&session) {
        return Ok(());
    }

    // Extract git context from session's working directory
    let git = working_directory(&session)
        .map(extract_git_context)
        .unwrap_or_default();

    // Upsert to local DB
    let db = LocalDb::open()?;
    db.upsert_local_session(&session, &file_path_str, &git)?;

    // Save updated offset
    state.byte_offset = file_len;
    save_state(&sp, &state)?;

    Ok(())
}

// ── Hook management ──────────────────────────────────────────────────────

/// Enable stream-write for an agent by installing the PostToolUse hook.
pub fn enable_stream_write(agent: &str) -> Result<()> {
    match agent {
        "claude-code" => enable_claude_code_hook(),
        _ => bail!("Unsupported agent: {agent}"),
    }
}

fn claude_settings_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

const HOOK_MATCHER: &str = "Edit|Write|Bash|NotebookEdit";
const HOOK_COMMAND: &str = "opensession daemon stream-push --agent claude-code";

fn enable_claude_code_hook() -> Result<()> {
    let settings_path = claude_settings_path()?;

    // Read or create settings
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Build the hook entry
    let hook_entry = serde_json::json!({
        "matcher": HOOK_MATCHER,
        "hooks": [{
            "type": "command",
            "command": HOOK_COMMAND,
            "timeout": 10,
            "statusMessage": "Streaming session..."
        }]
    });

    // Get or create hooks.PostToolUse array
    let hooks = settings
        .as_object_mut()
        .context("settings is not an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let post_tool = hooks
        .as_object_mut()
        .context("hooks is not an object")?
        .entry("PostToolUse")
        .or_insert_with(|| serde_json::json!([]));
    let arr = post_tool
        .as_array_mut()
        .context("PostToolUse is not an array")?;

    // Check if already installed
    let already_installed = arr.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .is_some_and(|hooks| {
                hooks
                    .iter()
                    .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND))
            })
    });

    if already_installed {
        println!("Stream-write hook already installed for claude-code.");
        return Ok(());
    }

    arr.push(hook_entry);

    // Write settings back
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&settings_path, content)?;

    println!("Stream-write enabled for claude-code.");
    println!("Hook installed in {}", settings_path.display());
    Ok(())
}
