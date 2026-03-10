use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Metadata about a discovered session location for a specific AI tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLocation {
    pub tool: String,
    pub paths: Vec<PathBuf>,
}

/// Discover local session files from known paths for all supported AI tools.
#[must_use]
pub fn discover_sessions() -> Vec<SessionLocation> {
    discover_sessions_from_home(&dirs_home())
}

/// Discover sessions for a specific tool by name.
#[must_use]
pub fn discover_for_tool(tool: &str) -> Vec<PathBuf> {
    discover_for_tool_in(&dirs_home(), tool)
}

/// Discover sessions matching an external parser's glob pattern.
#[must_use]
pub fn discover_external(glob_pattern: &str) -> Vec<PathBuf> {
    let expanded = shellexpand::tilde(glob_pattern).to_string();
    glob::glob(&expanded)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn discover_sessions_from_home(home: &Path) -> Vec<SessionLocation> {
    let mut locations = Vec::new();

    discover_claude_code(home, &mut locations);
    discover_codex(home, &mut locations);
    discover_opencode(home, &mut locations);
    discover_cline(home, &mut locations);
    discover_amp(home, &mut locations);
    discover_cursor(home, &mut locations);
    discover_gemini(home, &mut locations);

    locations
}

fn discover_for_tool_in(home: &Path, tool: &str) -> Vec<PathBuf> {
    match tool {
        "claude-code" => find_files_with_ext(&home.join(".claude").join("projects"), "jsonl")
            .into_iter()
            .filter(|path| !is_auxiliary_session_path(path))
            .collect(),
        "codex" => find_codex_sessions(home),
        "opencode" => find_opencode_sessions(home),
        "cline" => find_cline_sessions(home),
        "amp" => find_amp_threads(home),
        "cursor" => find_cursor_vscdb(home),
        "gemini" => find_gemini_sessions(home),
        _ => Vec::new(),
    }
}

fn discover_claude_code(home: &Path, locations: &mut Vec<SessionLocation>) {
    let claude_path = home.join(".claude").join("projects");
    if !claude_path.exists() {
        return;
    }
    let paths: Vec<_> = find_files_with_ext(&claude_path, "jsonl")
        .into_iter()
        .filter(|path| !is_auxiliary_session_path(path))
        .collect();
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "claude-code".to_string(),
            paths,
        });
    }
}

fn discover_codex(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_codex_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "codex".to_string(),
            paths,
        });
    }
}

fn discover_opencode(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_opencode_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "opencode".to_string(),
            paths,
        });
    }
}

fn discover_cline(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_cline_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "cline".to_string(),
            paths,
        });
    }
}

fn discover_amp(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_amp_threads(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "amp".to_string(),
            paths,
        });
    }
}

fn discover_cursor(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_cursor_vscdb(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "cursor".to_string(),
            paths,
        });
    }
}

fn discover_gemini(home: &Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_gemini_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "gemini".to_string(),
            paths,
        });
    }
}

fn dirs_home() -> PathBuf {
    opensession_paths::home_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn find_files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let pattern = format!("{}/**/*.{}", dir.display(), ext);
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn find_codex_sessions(home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let codex_home = codex_home.trim();
        if !codex_home.is_empty() {
            roots.push(PathBuf::from(codex_home).join("sessions"));
        }
    }
    roots.push(home.join(".codex").join("sessions"));

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for path in find_files_with_ext(&root, "jsonl") {
            if !is_codex_rollout_session_file(&path) {
                continue;
            }
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }
    out
}

fn is_codex_rollout_session_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower == "rollout.jsonl" || (lower.starts_with("rollout-") && lower.ends_with(".jsonl"))
        })
        .unwrap_or(false)
}

fn find_opencode_sessions(home: &Path) -> Vec<PathBuf> {
    let session_path = home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("storage")
        .join("session");
    if !session_path.exists() {
        return Vec::new();
    }
    let pattern = format!("{}/*/*.json", session_path.display());
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn find_cline_sessions(home: &Path) -> Vec<PathBuf> {
    let tasks_dir = home.join(".cline").join("data").join("tasks");
    if !tasks_dir.exists() {
        return Vec::new();
    }
    let pattern = format!("{}/*/api_conversation_history.json", tasks_dir.display());
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn find_amp_threads(home: &Path) -> Vec<PathBuf> {
    let threads_dir = home
        .join(".local")
        .join("share")
        .join("amp")
        .join("threads");
    if !threads_dir.exists() {
        return Vec::new();
    }
    let pattern = format!("{}/*.json", threads_dir.display());
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn find_gemini_sessions(home: &Path) -> Vec<PathBuf> {
    let gemini_path = home.join(".gemini").join("tmp");
    if !gemini_path.exists() {
        return Vec::new();
    }
    let mut results = Vec::new();
    for ext in &["json", "jsonl"] {
        let pattern = format!("{}/*/chats/session-*.{}", gemini_path.display(), ext);
        if let Ok(paths) = glob::glob(&pattern) {
            results.extend(paths.filter_map(Result::ok));
        }
    }
    results
}

fn find_cursor_vscdb(home: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();

    let cursor_base = home
        .join("Library")
        .join("Application Support")
        .join("Cursor")
        .join("User");
    let cursor_base_linux = home.join(".config").join("Cursor").join("User");

    for base in &[&cursor_base, &cursor_base_linux] {
        if !base.exists() {
            continue;
        }

        let global_db = base.join("globalStorage").join("state.vscdb");
        if global_db.exists() && cursor_db_has_composer_data(&global_db) {
            results.push(global_db);
        }

        let workspace_dir = base.join("workspaceStorage");
        if workspace_dir.exists() {
            let pattern = format!("{}/*/state.vscdb", workspace_dir.display());
            if let Ok(paths) = glob::glob(&pattern) {
                results.extend(
                    paths
                        .filter_map(Result::ok)
                        .filter(|path| cursor_db_has_composer_data(path)),
                );
            }
        }
    }

    results
}

fn cursor_db_has_composer_data(path: &Path) -> bool {
    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(_) => return false,
    };

    let mut found = false;
    if table_exists(&conn, "cursorDiskKV") {
        found |= has_cursor_rows(&conn, "cursorDiskKV");
    }
    if table_exists(&conn, "ItemTable") {
        found |= has_cursor_rows(&conn, "ItemTable");
    }
    found
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

fn has_cursor_rows(conn: &Connection, table: &str) -> bool {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {table} \
         WHERE key LIKE 'composerData:%' \
            OR key = 'composer.composerData' \
         LIMIT 1)"
    );
    conn.query_row(&sql, [], |row| row.get(0)).unwrap_or(false)
}

fn is_auxiliary_session_path(path: &Path) -> bool {
    let path_text = path.to_string_lossy();
    if path_text.contains("/subagents/") || path_text.contains("\\subagents\\") {
        return true;
    }

    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.starts_with("agent-")
        || lower.starts_with("agent_")
        || lower.starts_with("subagent-")
        || lower.starts_with("subagent_")
}

#[cfg(test)]
mod tests {
    use super::{
        SessionLocation, discover_for_tool_in, discover_sessions_from_home, find_codex_sessions,
        is_codex_rollout_session_file,
    };
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarRestore {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarRestore {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                previous: std::env::var(key).ok(),
            }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                set_env_for_test(self.key, previous);
            } else {
                remove_env_for_test(self.key);
            }
        }
    }

    fn set_env_for_test(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: tests hold env_test_lock() while mutating process environment.
        unsafe { std::env::set_var(key, value) };
    }

    fn remove_env_for_test(key: &str) {
        // SAFETY: tests hold env_test_lock() while mutating process environment.
        unsafe { std::env::remove_var(key) };
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn write_cursor_fixture_db(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create cursor parent");
        }
        let conn = Connection::open(path).expect("create cursor db");
        conn.execute(
            "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .expect("create cursorDiskKV");
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            (
                "composerData:test",
                r#"{"composerId":"comp-1","conversation":[{"type":1,"text":"hello"}]}"#,
            ),
        )
        .expect("insert composer row");
    }

    fn collect_tools(locations: &[SessionLocation]) -> Vec<&str> {
        locations
            .iter()
            .map(|location| location.tool.as_str())
            .collect()
    }

    #[test]
    fn codex_rollout_matcher_only_accepts_rollout_files() {
        assert!(is_codex_rollout_session_file(Path::new(
            "/tmp/rollout-123.jsonl"
        )));
        assert!(is_codex_rollout_session_file(Path::new(
            "/tmp/rollout.jsonl"
        )));
        assert!(!is_codex_rollout_session_file(Path::new(
            "/tmp/summary.jsonl"
        )));
        assert!(!is_codex_rollout_session_file(Path::new(
            "/tmp/rollout-not-json.txt"
        )));
    }

    #[test]
    fn codex_discovery_ignores_non_rollout_jsonl() {
        let _guard = env_test_lock().lock().expect("env lock");
        let restore = EnvVarRestore::capture("CODEX_HOME");
        let root = unique_temp_dir("opensession-codex-discover");
        let sessions_dir = root.join("sessions").join("2026").join("02").join("20");
        fs::create_dir_all(&sessions_dir).expect("mkdir");

        fs::write(sessions_dir.join("rollout-1.jsonl"), "{}\n").expect("rollout");
        fs::write(sessions_dir.join("rollout.jsonl"), "{}\n").expect("rollout base");
        fs::write(sessions_dir.join("summary.jsonl"), "{}\n").expect("summary");
        fs::write(sessions_dir.join("notes.jsonl"), "{}\n").expect("notes");

        set_env_for_test("CODEX_HOME", &root);
        let found = find_codex_sessions(Path::new("/this/home/path/does/not/exist"));

        assert!(
            found
                .iter()
                .any(|path| path.ends_with(Path::new("rollout-1.jsonl")))
        );
        assert!(
            found
                .iter()
                .any(|path| path.ends_with(Path::new("rollout.jsonl")))
        );
        assert!(
            !found
                .iter()
                .any(|path| path.ends_with(Path::new("summary.jsonl")))
        );
        assert!(
            !found
                .iter()
                .any(|path| path.ends_with(Path::new("notes.jsonl")))
        );

        fs::remove_dir_all(&root).ok();
        drop(restore);
    }

    #[test]
    fn discover_sessions_preserves_tool_order() {
        let home = unique_temp_dir("opensession-discovery-order");
        let claude_dir = home.join(".claude/projects/demo");
        fs::create_dir_all(claude_dir.join("subagents")).expect("create claude dir");
        fs::write(claude_dir.join("session.jsonl"), "{}\n").expect("write claude");
        fs::write(claude_dir.join("subagents/agent-1.jsonl"), "{}\n").expect("write subagent");

        let codex_dir = home.join(".codex/sessions/2026/02/20");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(codex_dir.join("rollout.jsonl"), "{}\n").expect("write codex");

        let opencode_dir = home.join(".local/share/opencode/storage/session/project");
        fs::create_dir_all(&opencode_dir).expect("create opencode dir");
        fs::write(opencode_dir.join("ses.json"), "{}\n").expect("write opencode");

        let cline_dir = home.join(".cline/data/tasks/task-1");
        fs::create_dir_all(&cline_dir).expect("create cline dir");
        fs::write(cline_dir.join("api_conversation_history.json"), "{}\n").expect("write cline");

        let amp_dir = home.join(".local/share/amp/threads");
        fs::create_dir_all(&amp_dir).expect("create amp dir");
        fs::write(amp_dir.join("T-1.json"), "{}\n").expect("write amp");

        let cursor_db = home.join(".config/Cursor/User/workspaceStorage/test/state.vscdb");
        write_cursor_fixture_db(&cursor_db);

        let gemini_dir = home.join(".gemini/tmp/demo/chats");
        fs::create_dir_all(&gemini_dir).expect("create gemini dir");
        fs::write(gemini_dir.join("session-demo.json"), "{}\n").expect("write gemini");

        let locations = discover_sessions_from_home(&home);
        assert_eq!(
            collect_tools(&locations),
            vec![
                "claude-code",
                "codex",
                "opencode",
                "cline",
                "amp",
                "cursor",
                "gemini"
            ]
        );
        assert_eq!(locations[0].paths.len(), 1);
    }

    #[test]
    fn discover_for_tool_filters_auxiliary_claude_sessions() {
        let home = unique_temp_dir("opensession-discovery-claude");
        let project_dir = home.join(".claude/projects/demo");
        fs::create_dir_all(project_dir.join("subagents")).expect("create claude project");
        fs::write(project_dir.join("session.jsonl"), "{}\n").expect("write primary");
        fs::write(project_dir.join("subagents/agent-123.jsonl"), "{}\n").expect("write agent");
        fs::write(project_dir.join("subagent-123.jsonl"), "{}\n").expect("write sibling");

        let found = discover_for_tool_in(&home, "claude-code");
        assert_eq!(found, vec![project_dir.join("session.jsonl")]);
    }
}
