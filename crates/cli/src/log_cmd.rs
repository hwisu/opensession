use anyhow::Result;
use chrono::{Duration, Utc};
use opensession_local_db::{LocalDb, LocalSessionRow, LogFilter};

/// All available JSON fields for --json selection.
const AVAILABLE_JSON_FIELDS: &[&str] = &[
    "id",
    "tool",
    "model",
    "title",
    "description",
    "created_at",
    "duration_seconds",
    "message_count",
    "event_count",
    "total_input_tokens",
    "total_output_tokens",
    "has_errors",
    "files_modified",
    "working_directory",
    "git_repo_name",
    "source_path",
    "git_remote",
    "git_branch",
    "git_commit",
    "tags",
];

/// Run the `log` command.
#[allow(clippy::too_many_arguments)]
pub fn run_log(
    since: Option<&str>,
    before: Option<&str>,
    tool: Option<&str>,
    model: Option<&str>,
    touches: Option<&str>,
    grep: Option<&str>,
    has_errors: bool,
    project: Option<&str>,
    format: &crate::output::OutputFormat,
    limit: u32,
    json_fields: Option<&str>,
    jq_filter: Option<&str>,
) -> Result<()> {
    // Handle --json with no value: list available fields
    if let Some(fields) = json_fields {
        if fields.is_empty() {
            println!("Available fields for --json:");
            for field in AVAILABLE_JSON_FIELDS {
                println!("  {field}");
            }
            return Ok(());
        }
    }

    let db = LocalDb::open()?;

    let since_iso = since.map(parse_relative_time).transpose()?;
    let before_iso = before.map(parse_relative_time).transpose()?;

    // Auto-detect project from CWD if no explicit project filter.
    // Prefer git_repo_name (more robust), fall back to working_directory.
    let (working_dir, repo_name) = if project.is_some() {
        (project.map(String::from), None)
    } else {
        let repo = detect_git_repo_name();
        if repo.is_some() {
            (None, repo)
        } else {
            (detect_project_dir(), None)
        }
    };

    let filter = LogFilter {
        tool: tool.map(String::from),
        model: model.map(String::from),
        since: since_iso,
        before: before_iso,
        touches: touches.map(String::from),
        grep: grep.map(String::from),
        has_errors: if has_errors { Some(true) } else { None },
        working_directory: working_dir,
        git_repo_name: repo_name,
        limit: Some(limit),
        ..Default::default()
    };

    let sessions = db.list_sessions_log(&filter)?;

    if sessions.is_empty() {
        eprintln!("No sessions found. Run `opensession index` to build the index.");
        return Ok(());
    }

    // --json overrides --format
    if let Some(fields) = json_fields {
        let selected: Vec<&str> = fields.split(',').map(str::trim).collect();
        let entries: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                let full = session_to_full_json(s);
                select_json_fields(&full, &selected)
            })
            .collect();
        let json_str = serde_json::to_string_pretty(&entries)?;
        return apply_jq_filter(&json_str, jq_filter);
    }

    match format {
        crate::output::OutputFormat::Json => {
            let entries: Vec<serde_json::Value> =
                sessions.iter().map(session_to_full_json).collect();
            let json_str = serde_json::to_string_pretty(&entries)?;
            apply_jq_filter(&json_str, jq_filter)?;
        }
        crate::output::OutputFormat::Stream => {
            // Enveloped NDJSON — each line is a self-contained envelope
            for s in &sessions {
                let data = session_to_full_json(s);
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let envelope = crate::output::OutputEnvelope::new(
                    "session_log",
                    &format!("[{}] {}", s.tool, title),
                    data,
                );
                let json = serde_json::to_string(&envelope)?;
                println!("{json}");
            }
        }
        _ => {
            if jq_filter.is_some() {
                anyhow::bail!("--jq requires --format json, --format stream, or --json");
            }
            print_text(&sessions);
        }
    }

    Ok(())
}

fn print_text(sessions: &[LocalSessionRow]) {
    for s in sessions {
        let title = s
            .title
            .as_deref()
            .unwrap_or("(untitled)")
            .chars()
            .take(60)
            .collect::<String>();
        let model = s.agent_model.as_deref().unwrap_or("?");
        let duration = format_duration(s.duration_seconds);
        let tokens = format_tokens(s.total_input_tokens, s.total_output_tokens);
        let errors = if s.has_errors { " [ERR]" } else { "" };
        let id_short = if s.id.len() > 12 { &s.id[..12] } else { &s.id };

        println!(
            "\x1b[33m{id_short}\x1b[0m [{tool}] ({model}) {duration} {tokens}{errors}",
            tool = s.tool,
        );
        println!("    {title}");

        // Show time
        println!("    \x1b[2m{}\x1b[0m", &s.created_at);

        // Show files modified count
        if let Some(ref fm) = s.files_modified {
            if let Ok(files) = serde_json::from_str::<Vec<String>>(fm) {
                if !files.is_empty() {
                    let display: Vec<&str> = files.iter().map(|f| f.as_str()).take(5).collect();
                    let more = if files.len() > 5 {
                        format!(" +{} more", files.len() - 5)
                    } else {
                        String::new()
                    };
                    println!("    files: {}{more}", display.join(", "));
                }
            }
        }

        println!();
    }

    println!("\x1b[2mShowing {} session(s)\x1b[0m", sessions.len());
}

fn session_to_full_json(s: &LocalSessionRow) -> serde_json::Value {
    serde_json::json!({
        "id": s.id,
        "tool": s.tool,
        "model": s.agent_model,
        "title": s.title,
        "description": s.description,
        "created_at": s.created_at,
        "duration_seconds": s.duration_seconds,
        "message_count": s.message_count,
        "event_count": s.event_count,
        "total_input_tokens": s.total_input_tokens,
        "total_output_tokens": s.total_output_tokens,
        "has_errors": s.has_errors,
        "files_modified": s.files_modified.as_deref()
            .and_then(|f| serde_json::from_str::<Vec<String>>(f).ok()),
        "working_directory": s.working_directory,
        "git_repo_name": s.git_repo_name,
        "source_path": s.source_path,
        "git_remote": s.git_remote,
        "git_branch": s.git_branch,
        "git_commit": s.git_commit,
        "tags": s.tags,
    })
}

/// Apply a jq filter to JSON output.
/// Tries built-in implementation for common patterns, falls back to system jq.
fn apply_jq_filter(json: &str, jq_filter: Option<&str>) -> Result<()> {
    match jq_filter {
        Some(filter) => {
            let parsed: serde_json::Value = serde_json::from_str(json)?;

            // Try built-in filter first (no system dependency)
            if let Some(result) = builtin_jq(&parsed, filter) {
                for line in result {
                    println!("{line}");
                }
                return Ok(());
            }

            // Fall back to system jq
            pipe_to_system_jq(json, filter)
        }
        None => {
            println!("{json}");
            Ok(())
        }
    }
}

/// Built-in jq for common patterns. Returns None if the expression is too complex.
fn builtin_jq(value: &serde_json::Value, filter: &str) -> Option<Vec<String>> {
    let filter = filter.trim();

    // "." — identity
    if filter == "." {
        return Some(vec![serde_json::to_string_pretty(value).ok()?]);
    }

    // "length" — array/object length
    if filter == "length" {
        let len = match value {
            serde_json::Value::Array(a) => a.len(),
            serde_json::Value::Object(o) => o.len(),
            serde_json::Value::String(s) => s.len(),
            _ => return None,
        };
        return Some(vec![len.to_string()]);
    }

    // "keys" — object keys or array indices
    if filter == "keys" {
        let keys: Vec<serde_json::Value> = match value {
            serde_json::Value::Object(o) => o
                .keys()
                .map(|k| serde_json::Value::String(k.clone()))
                .collect(),
            serde_json::Value::Array(a) => (0..a.len())
                .map(|i| serde_json::Value::Number(i.into()))
                .collect(),
            _ => return None,
        };
        return Some(vec![serde_json::to_string_pretty(&keys).ok()?]);
    }

    // ".field" — top-level field access
    if filter.starts_with('.') && !filter.contains('[') && !filter.contains('|') {
        let path = &filter[1..];
        if path.is_empty() {
            return Some(vec![serde_json::to_string_pretty(value).ok()?]);
        }
        let result = traverse(value, path)?;
        return Some(vec![format_jq_value(result)]);
    }

    // ".[]" — iterate array
    if filter == ".[]" {
        if let serde_json::Value::Array(arr) = value {
            return Some(arr.iter().map(format_jq_value).collect());
        }
        return None;
    }

    // ".[].field" or ".[].field1.field2" — extract field from each array element
    if let Some(rest) = filter.strip_prefix(".[].") {
        if !rest.contains('[') && !rest.contains('|') {
            if let serde_json::Value::Array(arr) = value {
                let results: Vec<String> = arr
                    .iter()
                    .filter_map(|item| {
                        let v = traverse(item, rest)?;
                        Some(format_jq_value(v))
                    })
                    .collect();
                return Some(results);
            }
        }
        return None;
    }

    // ".[N]" — index into array
    if filter.starts_with(".[") && filter.ends_with(']') {
        let idx_str = &filter[2..filter.len() - 1];
        if let Ok(idx) = idx_str.parse::<usize>() {
            if let serde_json::Value::Array(arr) = value {
                if let Some(item) = arr.get(idx) {
                    return Some(vec![format_jq_value(item)]);
                }
                return Some(vec!["null".to_string()]);
            }
        }
        return None;
    }

    // Expression too complex for built-in
    None
}

/// Select specific fields from a JSON object, returning a new object with only those fields.
fn select_json_fields(full: &serde_json::Value, fields: &[&str]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for field in fields {
        if let Some(val) = full.get(*field) {
            map.insert(field.to_string(), val.clone());
        }
    }
    serde_json::Value::Object(map)
}

/// Traverse a JSON value by dot-separated path (e.g. "stats.message_count").
fn traverse<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

/// Format a JSON value for jq-style output (strings unquoted, others as JSON).
fn format_jq_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Pipe JSON through system jq.
fn pipe_to_system_jq(json: &str, filter: &str) -> Result<()> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new("jq")
        .arg(filter)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|_| {
            anyhow::anyhow!(
                "Complex jq expression requires system jq.\n\
             Install: brew install jq (macOS) / apt install jq (Linux)\n\
             Built-in filters: ., .field, .[], .[].field, .[N], length, keys"
            )
        })?;
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(json.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("jq exited with status {}", status);
    }
    Ok(())
}

/// Parse a relative time string like "3 hours ago", "2 days", "1 week" into an ISO8601 timestamp.
pub fn parse_relative_time(s: &str) -> Result<String> {
    let s = s.trim().trim_end_matches(" ago").trim();

    // Try parsing as ISO8601 directly
    if chrono::DateTime::parse_from_rfc3339(s).is_ok() {
        return Ok(s.to_string());
    }

    // Parse relative: "N unit"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 2 {
        if let Ok(n) = parts[0].parse::<i64>() {
            let unit = parts[1].trim_end_matches('s'); // normalize "hours" -> "hour"
            let duration = match unit {
                "minute" | "min" | "m" => Duration::minutes(n),
                "hour" | "hr" | "h" => Duration::hours(n),
                "day" | "d" => Duration::days(n),
                "week" | "w" => Duration::weeks(n),
                "month" => Duration::days(n * 30),
                _ => anyhow::bail!("Unknown time unit: '{}'", parts[1]),
            };
            let ts = Utc::now() - duration;
            return Ok(ts.to_rfc3339());
        }
    }

    // Single-word shortcuts
    match s {
        "today" => {
            let today = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
            let ts = today.and_utc();
            return Ok(ts.to_rfc3339());
        }
        "yesterday" => {
            let yesterday = (Utc::now() - Duration::days(1))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap();
            let ts = yesterday.and_utc();
            return Ok(ts.to_rfc3339());
        }
        _ => {}
    }

    anyhow::bail!(
        "Could not parse time '{}'. Use formats like '3 hours ago', '2 days', 'yesterday', or ISO8601.",
        s
    )
}

fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn format_tokens(input: i64, output: i64) -> String {
    let total = input + output;
    if total == 0 {
        return String::new();
    }
    if total < 1000 {
        format!("{total}tok")
    } else {
        format!("{:.1}K", total as f64 / 1000.0)
    }
}

/// Detect project directory from CWD by finding the git root.
fn detect_project_dir() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    // Walk up to find .git directory
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        dir = dir.parent()?;
    }
}

/// Detect git repo name from CWD's git remote.
fn detect_git_repo_name() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
    opensession_local_db::git::normalize_repo_name(&remote).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_time_iso8601() {
        let result = parse_relative_time("2024-01-01T00:00:00+00:00").unwrap();
        assert_eq!(result, "2024-01-01T00:00:00+00:00");
    }

    #[test]
    fn test_parse_relative_time_hours() {
        let result = parse_relative_time("3 hours").unwrap();
        // Should parse successfully and return a valid RFC3339 timestamp
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_hours_ago() {
        let result = parse_relative_time("3 hours ago").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_days() {
        let result = parse_relative_time("2 days").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_week() {
        let result = parse_relative_time("1 week").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_minutes() {
        let result = parse_relative_time("30 minutes").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_today() {
        let result = parse_relative_time("today").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_yesterday() {
        let result = parse_relative_time("yesterday").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn test_parse_relative_time_invalid() {
        assert!(parse_relative_time("garbage").is_err());
    }

    #[test]
    fn test_parse_relative_time_unknown_unit() {
        assert!(parse_relative_time("3 fortnights").is_err());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3599), "59m");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(7200), "2h 0m");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(0, 0), "");
        assert_eq!(format_tokens(500, 0), "500tok");
        assert_eq!(format_tokens(500, 500), "1.0K");
        assert_eq!(format_tokens(5000, 5000), "10.0K");
    }

    // ── builtin_jq tests ──────────────────────────────────────────────

    #[test]
    fn test_builtin_jq_identity() {
        let val = serde_json::json!({"a": 1});
        let result = builtin_jq(&val, ".").unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("\"a\""));
    }

    #[test]
    fn test_builtin_jq_length() {
        let val = serde_json::json!([1, 2, 3]);
        let result = builtin_jq(&val, "length").unwrap();
        assert_eq!(result, vec!["3"]);

        let val = serde_json::json!({"a": 1, "b": 2});
        let result = builtin_jq(&val, "length").unwrap();
        assert_eq!(result, vec!["2"]);
    }

    #[test]
    fn test_builtin_jq_keys() {
        let val = serde_json::json!({"b": 1, "a": 2});
        let result = builtin_jq(&val, "keys").unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("\"a\""));
        assert!(result[0].contains("\"b\""));
    }

    #[test]
    fn test_builtin_jq_field() {
        let val = serde_json::json!({"name": "test", "count": 42});
        let result = builtin_jq(&val, ".name").unwrap();
        assert_eq!(result, vec!["test"]);

        let result = builtin_jq(&val, ".count").unwrap();
        assert_eq!(result, vec!["42"]);
    }

    #[test]
    fn test_builtin_jq_nested_field() {
        let val = serde_json::json!({"stats": {"count": 5}});
        let result = builtin_jq(&val, ".stats.count").unwrap();
        assert_eq!(result, vec!["5"]);
    }

    #[test]
    fn test_builtin_jq_iterate_array() {
        let val = serde_json::json!([1, 2, 3]);
        let result = builtin_jq(&val, ".[]").unwrap();
        assert_eq!(result, vec!["1", "2", "3"]);
    }

    #[test]
    fn test_builtin_jq_array_field() {
        let val = serde_json::json!([
            {"name": "a"},
            {"name": "b"},
            {"name": "c"}
        ]);
        let result = builtin_jq(&val, ".[].name").unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_builtin_jq_index() {
        let val = serde_json::json!(["first", "second", "third"]);
        let result = builtin_jq(&val, ".[0]").unwrap();
        assert_eq!(result, vec!["first"]);

        let result = builtin_jq(&val, ".[2]").unwrap();
        assert_eq!(result, vec!["third"]);
    }

    #[test]
    fn test_builtin_jq_index_out_of_bounds() {
        let val = serde_json::json!([1]);
        let result = builtin_jq(&val, ".[99]").unwrap();
        assert_eq!(result, vec!["null"]);
    }

    #[test]
    fn test_builtin_jq_complex_falls_back() {
        let val = serde_json::json!([1, 2, 3]);
        // Pipe expression — too complex for built-in
        assert!(builtin_jq(&val, ".[] | select(. > 1)").is_none());
        // Map — too complex
        assert!(builtin_jq(&val, "[.[] | . * 2]").is_none());
    }

    // ── session_to_full_json tests ──────────────────────────────────────

    fn make_test_row() -> opensession_local_db::LocalSessionRow {
        opensession_local_db::LocalSessionRow {
            id: "abc-123".to_string(),
            source_path: Some("/tmp/session.jsonl".to_string()),
            sync_status: "pending".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: None,
            tool: "claude-code".to_string(),
            agent_provider: Some("anthropic".to_string()),
            agent_model: Some("opus".to_string()),
            title: Some("Fix bug".to_string()),
            description: Some("Fixed a nasty bug".to_string()),
            tags: Some("rust,cli".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 10,
            user_message_count: 5,
            task_count: 2,
            event_count: 30,
            duration_seconds: 300,
            total_input_tokens: 5000,
            total_output_tokens: 3000,
            git_remote: Some("git@github.com:user/repo.git".to_string()),
            git_branch: Some("main".to_string()),
            git_commit: Some("abc123".to_string()),
            git_repo_name: Some("user/repo".to_string()),
            pr_number: None,
            pr_url: None,
            working_directory: Some("/home/user/project".to_string()),
            files_modified: Some(r#"["src/main.rs","src/lib.rs"]"#.to_string()),
            files_read: Some("README.md".to_string()),
            has_errors: false,
            max_active_agents: 1,
        }
    }

    #[test]
    fn test_session_to_full_json_all_fields() {
        let row = make_test_row();
        let json = session_to_full_json(&row);
        assert_eq!(json["id"], "abc-123");
        assert_eq!(json["tool"], "claude-code");
        assert_eq!(json["model"], "opus");
        assert_eq!(json["title"], "Fix bug");
        assert_eq!(json["duration_seconds"], 300);
        assert_eq!(json["message_count"], 10);
        assert_eq!(json["total_input_tokens"], 5000);
        assert_eq!(json["has_errors"], false);
        assert_eq!(json["git_repo_name"], "user/repo");
    }

    #[test]
    fn test_session_to_full_json_optional_none() {
        let mut row = make_test_row();
        row.title = None;
        row.agent_model = None;
        row.git_remote = None;
        let json = session_to_full_json(&row);
        assert!(json["title"].is_null());
        assert!(json["model"].is_null());
        assert!(json["git_remote"].is_null());
    }

    #[test]
    fn test_session_to_full_json_files_modified_parsed() {
        let row = make_test_row();
        let json = session_to_full_json(&row);
        let files = json["files_modified"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "src/main.rs");
        assert_eq!(files[1], "src/lib.rs");
    }

    #[test]
    fn test_session_to_full_json_files_modified_invalid() {
        let mut row = make_test_row();
        row.files_modified = Some("not valid json".to_string());
        let json = session_to_full_json(&row);
        // Invalid JSON → null (from_str fails, and_then returns None)
        assert!(json["files_modified"].is_null());
    }

    // ── select_json_fields tests ────────────────────────────────────────

    #[test]
    fn test_select_json_fields_single() {
        let full = serde_json::json!({"id": "abc", "tool": "claude", "model": "opus"});
        let result = select_json_fields(&full, &["id"]);
        assert_eq!(result, serde_json::json!({"id": "abc"}));
    }

    #[test]
    fn test_select_json_fields_multiple() {
        let full = serde_json::json!({"id": "abc", "tool": "claude", "model": "opus"});
        let result = select_json_fields(&full, &["id", "tool"]);
        assert_eq!(result, serde_json::json!({"id": "abc", "tool": "claude"}));
    }

    #[test]
    fn test_select_json_fields_nonexistent() {
        let full = serde_json::json!({"id": "abc"});
        let result = select_json_fields(&full, &["missing"]);
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn test_select_json_fields_mixed() {
        let full = serde_json::json!({"id": "abc", "tool": "claude"});
        let result = select_json_fields(&full, &["id", "missing", "tool"]);
        assert_eq!(result, serde_json::json!({"id": "abc", "tool": "claude"}));
    }

    // ── traverse tests ──────────────────────────────────────────────────

    #[test]
    fn test_traverse_single_key() {
        let val = serde_json::json!({"name": "test"});
        assert_eq!(traverse(&val, "name").unwrap(), &serde_json::json!("test"));
    }

    #[test]
    fn test_traverse_nested() {
        let val = serde_json::json!({"a": {"b": {"c": 42}}});
        assert_eq!(traverse(&val, "a.b.c").unwrap(), &serde_json::json!(42));
    }

    #[test]
    fn test_traverse_missing() {
        let val = serde_json::json!({"name": "test"});
        assert!(traverse(&val, "missing").is_none());
    }

    #[test]
    fn test_traverse_partial_path() {
        let val = serde_json::json!({"a": {"b": 1}});
        assert!(traverse(&val, "a.b.c").is_none());
    }

    // ── format_jq_value tests ───────────────────────────────────────────

    #[test]
    fn test_format_jq_value_string() {
        let v = serde_json::json!("hello");
        assert_eq!(format_jq_value(&v), "hello");
    }

    #[test]
    fn test_format_jq_value_null() {
        let v = serde_json::json!(null);
        assert_eq!(format_jq_value(&v), "null");
    }

    #[test]
    fn test_format_jq_value_number() {
        let v = serde_json::json!(42);
        assert_eq!(format_jq_value(&v), "42");
    }

    #[test]
    fn test_format_jq_value_bool() {
        let v = serde_json::json!(true);
        assert_eq!(format_jq_value(&v), "true");
    }
}
