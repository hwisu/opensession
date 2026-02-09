use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use dialoguer::Select;
use opensession_core::extract::{extract_first_user_text, truncate_str};
use opensession_core::{ContentBlock, EventType, Session};
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};

/// Run the handoff command: parse a session file and output a structured summary.
pub fn run_handoff(file: Option<&Path>, last: bool, output: Option<&Path>) -> Result<()> {
    let resolved = resolve_session_file(file, last)?;

    let parsers = all_parsers();
    let parser: Option<&dyn SessionParser> = parsers
        .iter()
        .find(|p| p.can_parse(&resolved))
        .map(|p| p.as_ref());

    let parser = match parser {
        Some(p) => p,
        None => bail!(
            "No parser found for file: {}\nSupported formats: Claude Code (.jsonl), OpenCode (.json), Goose (.db), Aider, Cursor",
            resolved.display()
        ),
    };

    let session = parser
        .parse(&resolved)
        .with_context(|| format!("Failed to parse {}", resolved.display()))?;

    let md = generate_handoff(&session);

    if let Some(out) = output {
        std::fs::write(out, &md)
            .with_context(|| format!("Failed to write {}", out.display()))?;
        println!("Handoff written to {}", out.display());
    } else {
        print!("{md}");
    }

    Ok(())
}

/// Resolve which session file to use: explicit path, --last, or interactive selection.
fn resolve_session_file(file: Option<&Path>, last: bool) -> Result<PathBuf> {
    // Explicit file path takes priority
    if let Some(f) = file {
        if !f.exists() {
            bail!("File not found: {}", f.display());
        }
        return Ok(f.to_path_buf());
    }

    // Discover all sessions and sort by modification time (newest first)
    let all = collect_all_session_paths()?;

    if all.is_empty() {
        bail!("No AI sessions found on this machine. Nothing to hand off.");
    }

    if last {
        let (path, tool) = &all[0];
        eprintln!("Using most recent session: [{}] {}", tool, path.display());
        return Ok(path.clone());
    }

    // Interactive selection
    let items: Vec<String> = all
        .iter()
        .map(|(path, tool)| {
            let modified = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or_else(|| "?".to_string());
            format!("[{}] {} ({})", tool, path.display(), modified)
        })
        .collect();

    let selection = Select::new()
        .with_prompt("Select a session")
        .items(&items)
        .default(0)
        .interact()?;

    Ok(all[selection].0.clone())
}

/// Collect all discovered session paths, sorted by modification time (newest first).
fn collect_all_session_paths() -> Result<Vec<(PathBuf, String)>> {
    let locations = discover_sessions();
    let mut all: Vec<(PathBuf, String)> = Vec::new();

    for loc in locations {
        for path in loc.paths {
            all.push((path, loc.tool.clone()));
        }
    }

    // Sort by modification time, newest first
    all.sort_by(|(a, _), (b, _)| {
        let ma = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mb = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    Ok(all)
}

/// Generate a Markdown handoff summary from a parsed session.
fn generate_handoff(session: &Session) -> String {
    let objective = extract_first_user_text(session)
        .map(|t| truncate_str(&t, 200))
        .unwrap_or_else(|| "(no user message found)".to_string());

    // Collect data from events
    let mut files_modified: HashMap<String, &str> = HashMap::new(); // path → action
    let mut files_read: HashSet<String> = HashSet::new();
    let mut shell_commands: Vec<(String, Option<i32>)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut user_messages: Vec<String> = Vec::new();

    for event in &session.events {
        match &event.event_type {
            EventType::FileCreate { path } => {
                files_modified.insert(path.clone(), "created");
            }
            EventType::FileEdit { path, .. } => {
                // Don't overwrite "created" with "edited"
                files_modified.entry(path.clone()).or_insert("edited");
            }
            EventType::FileDelete { path } => {
                files_modified.insert(path.clone(), "deleted");
            }
            EventType::FileRead { path } => {
                files_read.insert(path.clone());
            }
            EventType::ShellCommand { command, exit_code } => {
                shell_commands.push((command.clone(), *exit_code));
                if *exit_code != Some(0) && exit_code.is_some() {
                    errors.push(format!(
                        "Shell: `{}` → exit {}",
                        truncate_str(command, 80),
                        exit_code.unwrap()
                    ));
                }
            }
            EventType::ToolResult {
                is_error: true,
                name,
                ..
            } => {
                // Try to extract path context from the event content
                let detail = extract_text_from_event(event);
                if let Some(detail) = detail {
                    errors.push(format!(
                        "Tool error: {} — {}",
                        name,
                        truncate_str(&detail, 80)
                    ));
                } else {
                    errors.push(format!("Tool error: {name}"));
                }
            }
            EventType::UserMessage => {
                for block in &event.content.blocks {
                    if let ContentBlock::Text { text } = block {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            user_messages.push(trimmed.to_string());
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Remove read-only files that were also modified
    for path in files_modified.keys() {
        files_read.remove(path);
    }

    // Build markdown
    let mut md = String::new();

    md.push_str("# Session Handoff\n\n");

    // Objective
    md.push_str("## Objective\n");
    md.push_str(&objective);
    md.push_str("\n\n");

    // Summary
    md.push_str("## Summary\n");
    md.push_str(&format!(
        "- **Tool:** {} ({})\n",
        session.agent.tool, session.agent.model
    ));
    md.push_str(&format!(
        "- **Duration:** {}\n",
        format_duration(session.stats.duration_seconds)
    ));
    md.push_str(&format!(
        "- **Messages:** {} | Tool calls: {} | Events: {}\n",
        session.stats.message_count, session.stats.tool_call_count, session.stats.event_count
    ));
    md.push('\n');

    // Files Modified
    if !files_modified.is_empty() {
        md.push_str("## Files Modified\n");
        let mut paths: Vec<_> = files_modified.iter().collect();
        paths.sort_by_key(|(p, _)| p.as_str());
        for (path, action) in paths {
            md.push_str(&format!("- `{path}` ({action})\n"));
        }
        md.push('\n');
    }

    // Files Read
    if !files_read.is_empty() {
        md.push_str("## Files Read\n");
        let mut paths: Vec<_> = files_read.iter().collect();
        paths.sort();
        for path in paths {
            md.push_str(&format!("- `{path}`\n"));
        }
        md.push('\n');
    }

    // Shell Commands
    if !shell_commands.is_empty() {
        md.push_str("## Shell Commands\n");
        for (cmd, exit_code) in &shell_commands {
            let code_str = match exit_code {
                Some(c) => c.to_string(),
                None => "?".to_string(),
            };
            md.push_str(&format!(
                "- `{}` → {}\n",
                truncate_str(cmd, 80),
                code_str
            ));
        }
        md.push('\n');
    }

    // Errors
    if !errors.is_empty() {
        md.push_str("## Errors\n");
        for err in &errors {
            md.push_str(&format!("- {err}\n"));
        }
        md.push('\n');
    }

    // Key Conversations
    if !user_messages.is_empty() {
        md.push_str("## Key Conversations\n");
        for (i, msg) in user_messages.iter().enumerate() {
            md.push_str(&format!("{}. {}\n", i + 1, truncate_str(msg, 150)));
        }
        md.push('\n');
    }

    md
}

/// Extract the first text block from an event's content.
fn extract_text_from_event(event: &opensession_core::Event) -> Option<String> {
    for block in &event.content.blocks {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Format seconds into a human-readable duration string.
fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let m = seconds / 60;
        let s = seconds % 60;
        format!("{m}m {s}s")
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        let s = seconds % 60;
        format!("{h}h {m}m {s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opensession_core::{Agent, Content, Event, Session, Stats};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_agent() -> Agent {
        Agent {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-6".to_string(),
            tool: "claude-code".to_string(),
            tool_version: None,
        }
    }

    fn make_event(event_type: EventType, text: &str) -> Event {
        Event {
            event_id: "e1".to_string(),
            timestamp: Utc::now(),
            event_type,
            task_id: None,
            content: Content::text(text),
            duration_ms: None,
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(750), "12m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_generate_handoff_basic() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.stats = Stats {
            event_count: 10,
            message_count: 3,
            tool_call_count: 5,
            task_count: 0,
            duration_seconds: 750,
        };
        session.events.push(make_event(EventType::UserMessage, "Fix the build error"));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "Cargo.toml".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo build".to_string(),
                exit_code: Some(0),
            },
            "",
        ));

        let md = generate_handoff(&session);

        assert!(md.contains("# Session Handoff"));
        assert!(md.contains("Fix the build error"));
        assert!(md.contains("claude-code (claude-opus-4-6)"));
        assert!(md.contains("12m 30s"));
        assert!(md.contains("`src/main.rs` (edited)"));
        assert!(md.contains("`Cargo.toml`"));
        assert!(md.contains("`cargo build` → 0"));
    }

    #[test]
    fn test_files_read_excludes_modified() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.events.push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileRead { path: "src/main.rs".to_string() },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit { path: "src/main.rs".to_string(), diff: None },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead { path: "README.md".to_string() },
            "",
        ));

        let md = generate_handoff(&session);

        // Files Read should only show README.md, not src/main.rs
        assert!(md.contains("## Files Read\n- `README.md`"));
        assert!(!md.contains("## Files Read") || !md.contains("Files Read\n- `src/main.rs`"));
    }

    #[test]
    fn test_file_create_not_overwritten_by_edit() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.events.push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileCreate { path: "new_file.rs".to_string() },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit { path: "new_file.rs".to_string(), diff: None },
            "",
        ));

        let md = generate_handoff(&session);
        assert!(md.contains("`new_file.rs` (created)"));
    }

    #[test]
    fn test_shell_error_in_errors_section() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session.events.push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let md = generate_handoff(&session);
        assert!(md.contains("## Errors"));
        assert!(md.contains("Shell: `cargo test` → exit 1"));
    }
}
