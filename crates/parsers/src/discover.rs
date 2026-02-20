use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::path::PathBuf;

/// Metadata about a discovered session location for a specific AI tool.
pub struct SessionLocation {
    pub tool: String,
    pub paths: Vec<PathBuf>,
}

/// Discover local session files from known paths for all supported AI tools.
pub fn discover_sessions() -> Vec<SessionLocation> {
    let home = dirs_home();
    let mut locations = Vec::new();

    discover_claude_code(&home, &mut locations);
    discover_codex(&home, &mut locations);
    discover_opencode(&home, &mut locations);
    discover_cline(&home, &mut locations);
    discover_amp(&home, &mut locations);
    discover_cursor(&home, &mut locations);
    discover_gemini(&home, &mut locations);

    locations
}

/// Discover sessions for a specific tool by name.
pub fn discover_for_tool(tool: &str) -> Vec<PathBuf> {
    let home = dirs_home();
    match tool {
        "claude-code" => find_files_with_ext(&home.join(".claude").join("projects"), "jsonl")
            .into_iter()
            .filter(|p| !crate::is_auxiliary_session_path(p))
            .collect(),
        "codex" => find_codex_sessions(&home),
        "opencode" => find_opencode_sessions(&home),
        "cline" => find_cline_sessions(&home),
        "amp" => find_amp_threads(&home),
        "cursor" => find_cursor_vscdb(&home),
        "gemini" => find_gemini_sessions(&home),
        _ => Vec::new(),
    }
}

// ── Per-tool discovery ──────────────────────────────────────────────────────

fn discover_claude_code(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let claude_path = home.join(".claude").join("projects");
    if !claude_path.exists() {
        return;
    }
    let paths: Vec<_> = find_files_with_ext(&claude_path, "jsonl")
        .into_iter()
        .filter(|p| !crate::is_auxiliary_session_path(p))
        .collect();
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "claude-code".to_string(),
            paths,
        });
    }
}

fn discover_codex(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_codex_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "codex".to_string(),
            paths,
        });
    }
}

fn discover_opencode(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_opencode_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "opencode".to_string(),
            paths,
        });
    }
}

fn discover_cline(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_cline_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "cline".to_string(),
            paths,
        });
    }
}

fn discover_amp(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_amp_threads(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "amp".to_string(),
            paths,
        });
    }
}

fn discover_gemini(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_gemini_sessions(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "gemini".to_string(),
            paths,
        });
    }
}

fn discover_cursor(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_cursor_vscdb(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "cursor".to_string(),
            paths,
        });
    }
}

// ── Utilities ───────────────────────────────────────────────────────────────

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Recursively find files with a given extension under a directory.
fn find_files_with_ext(dir: &std::path::Path, ext: &str) -> Vec<PathBuf> {
    let pattern = format!("{}/**/*.{}", dir.display(), ext);
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Codex stores sessions as JSONL files under ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl
fn find_codex_sessions(home: &std::path::Path) -> Vec<PathBuf> {
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

fn is_codex_rollout_session_file(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower == "rollout.jsonl" || (lower.starts_with("rollout-") && lower.ends_with(".jsonl"))
        })
        .unwrap_or(false)
}

/// OpenCode stores session info as JSON files under
/// ~/.local/share/opencode/storage/session/<project_hash>/<session_id>.json
fn find_opencode_sessions(home: &std::path::Path) -> Vec<PathBuf> {
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

/// Cline stores sessions as task directories under ~/.cline/data/tasks/{taskId}/
/// Each task has api_conversation_history.json as the entry point.
fn find_cline_sessions(home: &std::path::Path) -> Vec<PathBuf> {
    let tasks_dir = home.join(".cline").join("data").join("tasks");
    if !tasks_dir.exists() {
        return Vec::new();
    }
    let pattern = format!("{}/*/api_conversation_history.json", tasks_dir.display());
    glob::glob(&pattern)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Amp stores threads as JSON files under ~/.local/share/amp/threads/T-{uuid}.json
fn find_amp_threads(home: &std::path::Path) -> Vec<PathBuf> {
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

/// Discover sessions matching an external parser's glob pattern.
pub fn discover_external(glob_pattern: &str) -> Vec<PathBuf> {
    let expanded = shellexpand::tilde(glob_pattern).to_string();
    glob::glob(&expanded)
        .map(|paths| paths.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// Gemini CLI stores sessions as JSON or JSONL files under
/// ~/.gemini/tmp/<project_hash>/chats/session-*.{json,jsonl}
fn find_gemini_sessions(home: &std::path::Path) -> Vec<PathBuf> {
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

/// Cursor stores conversation data in SQLite databases (state.vscdb).
/// Global: ~/Library/Application Support/Cursor/User/globalStorage/state.vscdb
/// Per-workspace: ~/Library/Application Support/Cursor/User/workspaceStorage/<hash>/state.vscdb
fn find_cursor_vscdb(home: &std::path::Path) -> Vec<PathBuf> {
    let mut results = Vec::new();

    // macOS path
    let cursor_base = home
        .join("Library")
        .join("Application Support")
        .join("Cursor")
        .join("User");

    // Linux path (XDG)
    let cursor_base_linux = home.join(".config").join("Cursor").join("User");

    for base in &[&cursor_base, &cursor_base_linux] {
        if !base.exists() {
            continue;
        }

        // Global state.vscdb
        let global_db = base.join("globalStorage").join("state.vscdb");
        if global_db.exists() && cursor_db_has_composer_data(&global_db) {
            results.push(global_db);
        }

        // Per-workspace state.vscdb files
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

fn cursor_db_has_composer_data(path: &std::path::Path) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{find_codex_sessions, is_codex_rollout_session_file};
    use std::path::Path;
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
            if let Some(ref previous) = self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
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
        let unique = format!(
            "opensession-codex-discover-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let sessions_dir = root.join("sessions").join("2026").join("02").join("20");
        std::fs::create_dir_all(&sessions_dir).expect("mkdir");

        std::fs::write(sessions_dir.join("rollout-1.jsonl"), "{}\n").expect("rollout");
        std::fs::write(sessions_dir.join("rollout.jsonl"), "{}\n").expect("rollout base");
        std::fs::write(sessions_dir.join("summary.jsonl"), "{}\n").expect("summary");
        std::fs::write(sessions_dir.join("notes.jsonl"), "{}\n").expect("notes");

        std::env::set_var("CODEX_HOME", &root);
        let found = find_codex_sessions(Path::new("/this/home/path/does/not/exist"));

        assert!(found
            .iter()
            .any(|path| path.ends_with(Path::new("rollout-1.jsonl"))));
        assert!(found
            .iter()
            .any(|path| path.ends_with(Path::new("rollout.jsonl"))));
        assert!(!found
            .iter()
            .any(|path| path.ends_with(Path::new("summary.jsonl"))));
        assert!(!found
            .iter()
            .any(|path| path.ends_with(Path::new("notes.jsonl"))));

        std::fs::remove_dir_all(&root).ok();
        drop(restore);
    }
}
