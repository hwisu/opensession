mod app;
mod config;
mod ui;
mod views;

use anyhow::Result;
use app::{App, ServerInfo, ServerStatus, StartupStatus, View};
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use opensession_local_db::git::extract_git_context;
use opensession_local_db::LocalDb;
use ratatui::prelude::*;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let sessions = if args.len() > 1 {
        // Load from explicit file paths
        load_from_paths(&args[1..])
    } else {
        // Auto-discover local sessions
        load_sessions()
    };

    let mut app = App::new(sessions);

    // ── Load full daemon config ──────────────────────────────────────
    let daemon_config = config::load_daemon_config();
    let config_exists = config::config_dir()
        .map(|d| d.join("daemon.toml").exists())
        .unwrap_or(false);

    // Build server info from daemon config
    app.server_info = build_server_info(&daemon_config);
    app.team_id = if daemon_config.identity.team_id.is_empty() {
        None
    } else {
        Some(daemon_config.identity.team_id.clone())
    };
    app.daemon_config = daemon_config;

    // ── Build startup status ─────────────────────────────────────────
    let mut status = StartupStatus {
        sessions_cached: 0,
        repos_detected: 0,
        daemon_pid: config::daemon_pid(),
        config_exists,
    };

    // ── Open local DB and cache sessions ─────────────────────────────
    if let Ok(db) = LocalDb::open() {
        let db = Arc::new(db);
        // Cache parsed sessions into the local DB
        cache_sessions_to_db(&db, &app.sessions);
        status.sessions_cached = app.sessions.len();
        // Load repo list for view cycling
        app.repos = db.list_repos().unwrap_or_default();
        status.repos_detected = app.repos.len();
        app.db = Some(db);
    }

    app.startup_status = status;

    // ── If config needs setup, start in Setup view ───────────────────
    if config::needs_setup(&app.daemon_config) {
        app.view = View::Setup;
        app.settings_index = 0;
    }

    // If targeting a local Docker server, do a health check
    if let Some(ref mut info) = app.server_info {
        if is_local_url(&info.url) {
            let url = info.url.clone();
            let rt = tokio::runtime::Runtime::new()?;
            info.status = rt.block_on(check_health(&url));
        }
    }

    // Terminal setup
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn is_local_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("localhost")
        || lower.contains("127.0.0.1")
        || lower.contains("192.168.")
        || lower.contains("10.")
        || lower.contains("172.16.")
}

fn build_server_info(config: &config::DaemonConfig) -> Option<ServerInfo> {
    if config.server.url.is_empty() {
        return None;
    }

    // Try to read last upload time from state.json (legacy)
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let state_path = PathBuf::from(&home)
        .join(".config")
        .join("opensession")
        .join("state.json");

    let last_upload = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("uploaded")?
                .as_object()?
                .values()
                .filter_map(|v| v.as_str().map(String::from))
                .max()
        });

    Some(ServerInfo {
        url: config.server.url.clone(),
        status: ServerStatus::Unknown,
        last_upload,
    })
}

async fn check_health(server_url: &str) -> ServerStatus {
    let url = format!("{}/api/health", server_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let version = body
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                ServerStatus::Online(version)
            } else {
                ServerStatus::Online("unknown".to_string())
            }
        }
        _ => ServerStatus::Offline,
    }
}

/// Cache parsed sessions into the local DB with git context.
fn cache_sessions_to_db(db: &LocalDb, sessions: &[opensession_core::trace::Session]) {
    for session in sessions {
        // Determine source path (best-effort: use session_id as key)
        let source = session.session_id.clone();

        // Extract git context from working directory
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str().map(String::from));
        let git = cwd
            .as_deref()
            .map(extract_git_context)
            .unwrap_or_default();

        let _ = db.upsert_local_session(session, &source, &git);
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if app.handle_key(key.code) {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Load sessions from explicit file paths passed as CLI args.
fn load_from_paths(args: &[String]) -> Vec<opensession_core::trace::Session> {
    let parsers = opensession_parsers::all_parsers();
    let mut sessions = Vec::new();

    for arg in args {
        let path = PathBuf::from(arg);
        if !path.exists() {
            eprintln!("Warning: file not found: {}", path.display());
            continue;
        }
        if let Some(parser) = parsers.iter().find(|p| p.can_parse(&path)) {
            match parser.parse(&path) {
                Ok(session) => sessions.push(session),
                Err(e) => eprintln!("Warning: failed to parse {}: {}", path.display(), e),
            }
        } else {
            eprintln!("Warning: no parser for {}", path.display());
        }
    }

    sessions.sort_by(|a, b| b.context.created_at.cmp(&a.context.created_at));
    sessions
}

/// Auto-discover sessions from known local paths.
fn load_sessions() -> Vec<opensession_core::trace::Session> {
    let locations = opensession_parsers::discover::discover_sessions();
    let parsers = opensession_parsers::all_parsers();
    let mut sessions = Vec::new();

    for location in &locations {
        for path in &location.paths {
            // Skip subagent session files
            let path_str = path.to_string_lossy();
            if path_str.contains("/subagents/") || path_str.contains("\\subagents\\") {
                continue;
            }
            if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
                if fname.starts_with("agent-") {
                    continue;
                }
            }

            if let Some(parser) = parsers.iter().find(|p| p.can_parse(path)) {
                if let Ok(session) = parser.parse(path) {
                    // Skip empty sessions (0 events usually means parse was incomplete)
                    if session.stats.event_count > 0 {
                        sessions.push(session);
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.context.created_at.cmp(&a.context.created_at));
    sessions
}
