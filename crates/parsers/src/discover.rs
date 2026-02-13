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
    discover_goose(&home, &mut locations);
    discover_aider(&home, &mut locations);
    discover_cursor(&home, &mut locations);
    discover_gemini(&home, &mut locations);

    locations
}

/// Discover sessions for a specific tool by name.
pub fn discover_for_tool(tool: &str) -> Vec<PathBuf> {
    let home = dirs_home();
    match tool {
        "claude-code" => find_files_with_ext(&home.join(".claude").join("projects"), "jsonl"),
        "codex" => find_codex_sessions(&home),
        "opencode" => find_opencode_sessions(&home),
        "cline" => find_cline_sessions(&home),
        "amp" => find_amp_threads(&home),
        "goose" => {
            let db_path = home
                .join(".local")
                .join("share")
                .join("goose")
                .join("sessions.db");
            if db_path.exists() {
                vec![db_path]
            } else {
                Vec::new()
            }
        }
        "aider" => find_aider_files(&home),
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
    let paths = find_files_with_ext(&claude_path, "jsonl");
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

fn discover_goose(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let goose_path = home
        .join(".local")
        .join("share")
        .join("goose")
        .join("sessions.db");
    if goose_path.exists() {
        locations.push(SessionLocation {
            tool: "goose".to_string(),
            paths: vec![goose_path],
        });
    }
}

fn discover_aider(home: &std::path::Path, locations: &mut Vec<SessionLocation>) {
    let paths = find_aider_files(home);
    if !paths.is_empty() {
        locations.push(SessionLocation {
            tool: "aider".to_string(),
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
    let codex_path = home.join(".codex").join("sessions");
    if !codex_path.exists() {
        return Vec::new();
    }
    find_files_with_ext(&codex_path, "jsonl")
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
        if global_db.exists() {
            results.push(global_db);
        }

        // Per-workspace state.vscdb files
        let workspace_dir = base.join("workspaceStorage");
        if workspace_dir.exists() {
            let pattern = format!("{}/*/state.vscdb", workspace_dir.display());
            if let Ok(paths) = glob::glob(&pattern) {
                results.extend(paths.filter_map(Result::ok));
            }
        }
    }

    results
}

/// Aider stores history in `.aider.chat.history.md` files in project directories.
/// We search common project locations.
fn find_aider_files(home: &std::path::Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    // Check home directory itself
    let home_aider = home.join(".aider.chat.history.md");
    if home_aider.exists() {
        results.push(home_aider);
    }
    // Check common project directories
    for dir_name in &["projects", "repos", "src", "code", "dev", "work"] {
        let base = home.join(dir_name);
        if !base.exists() {
            continue;
        }
        let pattern = format!("{}/**/.aider.chat.history.md", base.display());
        if let Ok(paths) = glob::glob(&pattern) {
            results.extend(paths.filter_map(Result::ok));
        }
    }
    results
}
