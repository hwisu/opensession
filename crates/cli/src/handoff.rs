use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use dialoguer::Select;
use opensession_core::handoff::HandoffSummary;
use opensession_core::Session;
use opensession_parsers::discover::discover_sessions;
use opensession_parsers::{all_parsers, SessionParser};

/// Run the handoff command: parse session file(s) and output a structured summary.
#[allow(clippy::too_many_arguments)]
pub async fn run_handoff(
    files: &[PathBuf],
    last: bool,
    output: Option<&Path>,
    format: crate::output::OutputFormat,
    summarize: bool,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
    _ai_provider: Option<&str>,
) -> Result<()> {
    let sessions = resolve_sessions(files, last, claude, gemini, tool_refs)?;

    if sessions.is_empty() {
        bail!("No sessions to process.");
    }

    // Use the unified output module
    let output_format = format;

    let mut result = Vec::new();
    crate::output::render_output(&sessions, &output_format, &mut result)?;
    let result_str = String::from_utf8(result)?;

    // Append LLM summary if requested (only for markdown output)
    let mut final_result = result_str;
    if summarize && output_format == crate::output::OutputFormat::Markdown {
        match crate::summarize::llm_summarize(&sessions).await {
            Ok(llm_summary) => {
                final_result.push_str("---\n\n## AI Summary\n\n");
                if !llm_summary.key_decisions.is_empty() {
                    final_result.push_str("### Key Decisions\n");
                    for d in &llm_summary.key_decisions {
                        final_result.push_str(&format!("- {d}\n"));
                    }
                    final_result.push('\n');
                }
                if !llm_summary.patterns_discovered.is_empty() {
                    final_result.push_str("### Patterns Discovered\n");
                    for p in &llm_summary.patterns_discovered {
                        final_result.push_str(&format!("- {p}\n"));
                    }
                    final_result.push('\n');
                }
                if !llm_summary.architecture_notes.is_empty() {
                    final_result.push_str("### Architecture Notes\n");
                    final_result.push_str(&llm_summary.architecture_notes);
                    final_result.push_str("\n\n");
                }
                if !llm_summary.next_steps.is_empty() {
                    final_result.push_str("### Suggested Next Steps\n");
                    for s in &llm_summary.next_steps {
                        final_result.push_str(&format!("- {s}\n"));
                    }
                    final_result.push('\n');
                }
            }
            Err(e) => {
                eprintln!("Warning: LLM summarize failed: {e}");
                final_result.push_str(&format!("\n---\n\n_LLM summary unavailable: {e}_\n"));
            }
        }
    }

    if let Some(out) = output {
        std::fs::write(out, &final_result)
            .with_context(|| format!("Failed to write {}", out.display()))?;
        eprintln!("Handoff written to {}", out.display());
    } else {
        print!("{final_result}");
    }

    Ok(())
}

/// Compare two sessions side-by-side.
pub async fn run_diff(session_a: &str, session_b: &str, _ai: bool) -> Result<()> {
    let parsers = all_parsers();

    let sa = resolve_single_session(session_a, &parsers)?;
    let sb = resolve_single_session(session_b, &parsers)?;

    let sum_a = HandoffSummary::from_session(&sa);
    let sum_b = HandoffSummary::from_session(&sb);

    println!(
        "Session A ({})     vs     Session B ({})",
        sum_a.tool, sum_b.tool
    );
    println!("{}", "─".repeat(60));
    println!(
        "Objective: {:40} {:40}",
        truncate(&sum_a.objective, 40),
        truncate(&sum_b.objective, 40),
    );
    println!("Model:     {:40} {:40}", sum_a.model, sum_b.model,);
    println!(
        "Duration:  {:40} {:40}",
        opensession_core::handoff::format_duration(sum_a.duration_seconds),
        opensession_core::handoff::format_duration(sum_b.duration_seconds),
    );
    println!(
        "Messages:  {:40} {:40}",
        format!("{}", sum_a.stats.message_count),
        format!("{}", sum_b.stats.message_count),
    );
    println!(
        "Tokens:    {:40} {:40}",
        format!(
            "{}in / {}out",
            sum_a.stats.total_input_tokens, sum_a.stats.total_output_tokens
        ),
        format!(
            "{}in / {}out",
            sum_b.stats.total_input_tokens, sum_b.stats.total_output_tokens
        ),
    );
    println!(
        "Errors:    {:40} {:40}",
        format!("{}", sum_a.errors.len()),
        format!("{}", sum_b.errors.len()),
    );

    // Files comparison
    println!("\nFiles Modified:");
    let files_a: std::collections::HashSet<&str> = sum_a
        .files_modified
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let files_b: std::collections::HashSet<&str> = sum_b
        .files_modified
        .iter()
        .map(|f| f.path.as_str())
        .collect();

    for f in &files_a {
        if files_b.contains(f) {
            println!("  {f} (both)");
        } else {
            println!("  {f} (A only)");
        }
    }
    for f in &files_b {
        if !files_a.contains(f) {
            println!("  {f} (B only)");
        }
    }

    Ok(())
}

/// Install the prepare-commit-msg git hook.
pub fn run_hooks_install() -> Result<()> {
    let git_dir = find_git_dir()?;
    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    let hook_path = hooks_dir.join("prepare-commit-msg");
    let hook_script = r#"#!/bin/sh
# opensession prepare-commit-msg hook
# Appends AI session info to commit messages

COMMIT_MSG_FILE="$1"
COMMIT_SOURCE="$2"

# Only run for regular commits (not merges, amends, etc.)
if [ -n "$COMMIT_SOURCE" ]; then
    exit 0
fi

# Try to find a matching session for staged files
SESSION_INFO=$(opensession log --limit 1 --format json 2>/dev/null | head -1)
if [ -z "$SESSION_INFO" ]; then
    exit 0
fi

# Use jq for safe JSON parsing (no shell injection)
if ! command -v jq &>/dev/null; then
    # Without jq, skip session info to avoid shell injection via grep/cut
    exit 0
fi

TOOL=$(printf '%s' "$SESSION_INFO" | jq -r '.[0].tool // empty' 2>/dev/null)
MODEL=$(printf '%s' "$SESSION_INFO" | jq -r '.[0].model // empty' 2>/dev/null)
TITLE=$(printf '%s' "$SESSION_INFO" | jq -r '.[0].title // empty' 2>/dev/null | head -c 80)

if [ -n "$TOOL" ]; then
    printf '\nSession: %s (%s)\n' "$TOOL" "$MODEL" >> "$COMMIT_MSG_FILE"
    if [ -n "$TITLE" ]; then
        printf 'Prompt: "%s"\n' "$TITLE" >> "$COMMIT_MSG_FILE"
    fi
fi
"#;

    std::fs::write(&hook_path, hook_script)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))?;
    }

    println!("Hook installed: {}", hook_path.display());
    Ok(())
}

/// Remove the prepare-commit-msg git hook.
pub fn run_hooks_uninstall() -> Result<()> {
    let git_dir = find_git_dir()?;
    let hook_path = git_dir.join("hooks").join("prepare-commit-msg");
    if hook_path.exists() {
        std::fs::remove_file(&hook_path)?;
        println!("Hook removed: {}", hook_path.display());
    } else {
        println!("No hook found to remove.");
    }
    Ok(())
}

fn find_git_dir() -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()?;
    if !output.status.success() {
        bail!("Not inside a git repository.");
    }
    let dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(dir))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

/// Resolve a single session from a string (ID, file path, or HEAD~N reference).
fn resolve_single_session(s: &str, parsers: &[Box<dyn SessionParser>]) -> Result<Session> {
    use crate::session_ref::SessionRef;

    let sref = SessionRef::parse(s);
    match sref {
        SessionRef::File(path) => parse_file(parsers, &path),
        SessionRef::Latest { .. } | SessionRef::Single { .. } | SessionRef::Id(_) => {
            let db = opensession_local_db::LocalDb::open()?;
            let row = sref.resolve_one(&db, None)?;
            let source = row
                .source_path
                .as_deref()
                .with_context(|| format!("Session {} has no source_path", row.id))?;
            parse_file(parsers, &PathBuf::from(source))
        }
    }
}

/// Resolve session files: explicit paths, --last, tool refs, or interactive selection.
fn resolve_sessions(
    files: &[PathBuf],
    last: bool,
    claude: Option<&str>,
    gemini: Option<&str>,
    tool_refs: &[String],
) -> Result<Vec<Session>> {
    use crate::session_ref::{tool_flag_to_name, SessionRef};

    let parsers = all_parsers();

    // If explicit files are given, parse them all
    if !files.is_empty() {
        let mut sessions = Vec::new();
        for file in files {
            if !file.exists() {
                bail!("File not found: {}", file.display());
            }
            let session = parse_file(&parsers, file)?;
            sessions.push(session);
        }
        return Ok(sessions);
    }

    // Collect tool-specific session refs
    let mut ref_pairs: Vec<(Option<&str>, SessionRef)> = Vec::new();

    if let Some(r) = claude {
        ref_pairs.push((Some("claude-code"), SessionRef::parse(r)));
    }
    if let Some(r) = gemini {
        ref_pairs.push((Some("gemini"), SessionRef::parse(r)));
    }
    for tool_ref_str in tool_refs {
        // Format: "tool_name ref" e.g. "amp HEAD~2"
        let parts: Vec<&str> = tool_ref_str.splitn(2, ' ').collect();
        if parts.len() == 2 {
            let tool_name = tool_flag_to_name(parts[0]);
            ref_pairs.push((Some(tool_name), SessionRef::parse(parts[1])));
        } else {
            ref_pairs.push((None, SessionRef::parse(parts[0])));
        }
    }

    // If we have session refs, resolve them via the index DB
    if !ref_pairs.is_empty() {
        let db = opensession_local_db::LocalDb::open()?;
        let mut sessions = Vec::new();
        for (tool, sref) in &ref_pairs {
            match sref {
                SessionRef::File(path) => {
                    sessions.push(parse_file(&parsers, path)?);
                }
                _ => {
                    let rows = sref.resolve(&db, *tool)?;
                    for row in &rows {
                        let source = row
                            .source_path
                            .as_deref()
                            .with_context(|| format!("Session {} has no source_path", row.id))?;
                        sessions.push(parse_file(&parsers, &PathBuf::from(source))?);
                    }
                }
            }
        }
        return Ok(sessions);
    }

    // Otherwise resolve a single file via --last or interactive
    let resolved = resolve_session_file(last)?;
    let session = parse_file(&parsers, &resolved)?;
    Ok(vec![session])
}

fn parse_file(parsers: &[Box<dyn SessionParser>], file: &Path) -> Result<Session> {
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

    parser
        .parse(file)
        .with_context(|| format!("Failed to parse {}", file.display()))
}

/// Resolve which session file to use: --last or interactive selection.
fn resolve_session_file(last: bool) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use opensession_core::handoff::{format_duration, generate_handoff_markdown, HandoffSummary};
    use opensession_core::testing;
    use opensession_core::{Agent, Event, EventType, Session, Stats};

    fn make_agent() -> Agent {
        testing::agent()
    }

    fn make_event(event_type: EventType, text: &str) -> Event {
        testing::event(event_type, text)
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
            ..Default::default()
        };
        session
            .events
            .push(make_event(EventType::UserMessage, "Fix the build error"));
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

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

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
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileRead {
                path: "src/main.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "src/main.rs".to_string(),
                diff: None,
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileRead {
                path: "README.md".to_string(),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);

        assert!(md.contains("## Files Read\n- `README.md`"));
    }

    #[test]
    fn test_file_create_not_overwritten_by_edit() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::FileCreate {
                path: "new_file.rs".to_string(),
            },
            "",
        ));
        session.events.push(make_event(
            EventType::FileEdit {
                path: "new_file.rs".to_string(),
                diff: None,
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);
        assert!(md.contains("`new_file.rs` (created)"));
    }

    #[test]
    fn test_shell_error_in_errors_section() {
        let mut session = Session::new("test-id".to_string(), make_agent());
        session
            .events
            .push(make_event(EventType::UserMessage, "test"));
        session.events.push(make_event(
            EventType::ShellCommand {
                command: "cargo test".to_string(),
                exit_code: Some(1),
            },
            "",
        ));

        let summary = HandoffSummary::from_session(&session);
        let md = generate_handoff_markdown(&summary);
        assert!(md.contains("## Errors"));
        assert!(md.contains("Shell: `cargo test` → exit 1"));
    }
}
