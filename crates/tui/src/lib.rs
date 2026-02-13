mod app;
mod async_ops;
mod config;
mod platform_api_storage;
mod theme;
mod ui;
mod views;

use anyhow::Result;
use app::{App, ServerInfo, ServerStatus, StartupStatus, UploadPhase, View};
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
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

enum BgEvent {
    SessionsLoaded(Vec<opensession_core::trace::Session>),
    DbReady { repos: Vec<String>, count: usize },
}

/// Launch the TUI. Optionally pass file paths to open specific sessions.
pub fn run(paths: Option<Vec<String>>) -> Result<()> {
    // Start with empty sessions — they'll load in the background
    let mut app = App::new(vec![]);
    app.loading_sessions = true;

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
    app.connection_ctx = App::derive_connection_ctx(&app.daemon_config);

    // ── Build startup status ─────────────────────────────────────────
    let status = StartupStatus {
        sessions_cached: 0,
        repos_detected: 0,
        daemon_pid: config::daemon_pid(),
        config_exists,
    };
    app.startup_status = status;

    // ── Open main DB connection (fast — just opens SQLite file) ──────
    if let Ok(db) = LocalDb::open() {
        app.db = Some(Arc::new(db));
    }

    // ── If config file doesn't exist yet, start in Setup view ──────
    if !config_exists {
        app.view = View::Setup;
        app.settings_index = 0;
    }

    // Terminal setup — show UI immediately
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // ── Spawn background session loading thread ─────────────────────
    let (tx, bg_rx) = mpsc::channel::<BgEvent>();

    std::thread::spawn(move || {
        let sessions = match paths {
            Some(ref paths) => load_from_paths(paths),
            None => load_sessions(),
        };
        let for_db = sessions.clone();
        if tx.send(BgEvent::SessionsLoaded(sessions)).is_err() {
            return;
        }

        // Separate DB connection for this thread (Connection is Send but not Sync)
        if let Ok(bg_db) = LocalDb::open() {
            cache_sessions_to_db(&bg_db, &for_db);
            let repos = bg_db.list_repos().unwrap_or_default();
            let _ = tx.send(BgEvent::DbReady {
                repos,
                count: for_db.len(),
            });
        }
    });

    // Main loop
    let result = event_loop(&mut terminal, &mut app, bg_rx);

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
    let client = match opensession_api_client::ApiClient::new(server_url, Duration::from_secs(1)) {
        Ok(c) => c,
        Err(_) => return ServerStatus::Offline,
    };
    match client.health().await {
        Ok(resp) => ServerStatus::Online(resp.version),
        Err(_) => ServerStatus::Offline,
    }
}

/// Cache parsed sessions into the local DB with git context.
/// Skips git extraction for sessions already in the DB (only updates stats).
fn cache_sessions_to_db(db: &LocalDb, sessions: &[opensession_core::trace::Session]) {
    let existing = db.existing_session_ids();

    for session in sessions {
        if existing.contains(&session.session_id) {
            // Already cached → only update stats (skip expensive git subprocess calls)
            let _ = db.update_session_stats(session);
            continue;
        }

        // New session → full insert with git context extraction
        let source = session.session_id.clone();
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str().map(String::from));
        let git = cwd.as_deref().map(extract_git_context).unwrap_or_default();

        let _ = db.upsert_local_session(session, &source, &git);
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    bg_rx: mpsc::Receiver<BgEvent>,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;

    loop {
        // ── Poll background session loading ──────────────────────────
        while let Ok(ev) = bg_rx.try_recv() {
            match ev {
                BgEvent::SessionsLoaded(sessions) => {
                    app.sessions = sessions;
                    app.filtered_sessions = (0..app.sessions.len()).collect();
                    if !app.sessions.is_empty() {
                        app.list_state.select(Some(0));
                    }
                    app.loading_sessions = false;
                }
                BgEvent::DbReady { repos, count } => {
                    app.repos = repos;
                    app.startup_status.sessions_cached = count;
                    app.startup_status.repos_detected = app.repos.len();
                }
            }
        }

        // ── Handle pending async command ─────────────────────────────
        if let Some(cmd) = app.pending_command.take() {
            let result = rt.block_on(async_ops::execute(cmd, &app.daemon_config));
            app.apply_command_result(result);
        }

        // ── Handle legacy upload popup async ops ─────────────────────
        // Login (triggered from Setup view)
        if app.login_state.loading {
            app.pending_command = Some(async_ops::AsyncCommand::Login {
                email: app.login_state.email.clone(),
                password: app.login_state.password.clone(),
            });
        }

        // Fetch teams for upload popup
        if let Some(ref popup) = app.upload_popup {
            if matches!(popup.phase, UploadPhase::FetchingTeams) {
                app.pending_command = Some(async_ops::AsyncCommand::FetchUploadTeams);
            }
        }

        // Upload session — sequential multi-target dispatch
        if let Some(ref popup) = app.upload_popup {
            if matches!(popup.phase, UploadPhase::Uploading) {
                // Find the next checked team that hasn't been uploaded yet
                let uploaded_names: Vec<_> =
                    popup.results.iter().map(|(name, _)| name.clone()).collect();
                let next_target = popup
                    .teams
                    .iter()
                    .enumerate()
                    .find(|(i, t)| popup.checked[*i] && !uploaded_names.contains(&t.name));

                if let Some((_idx, team)) = next_target {
                    let team_id = if team.is_personal {
                        None
                    } else {
                        Some(team.id.clone())
                    };
                    let team_name = team.name.clone();
                    let is_personal = team.is_personal;

                    let session_clone = app.selected_session().cloned();

                    if let Some(session) = session_clone {
                        let body_url = if is_personal {
                            try_git_store(&session, &app.daemon_config)
                        } else {
                            None
                        };

                        let session_json = serde_json::to_value(&session).ok();
                        if let Some(json) = session_json {
                            app.pending_command = Some(async_ops::AsyncCommand::UploadSession {
                                session_json: json,
                                team_id,
                                team_name,
                                body_url,
                            });
                        }
                    } else if let Some(ref mut popup) = app.upload_popup {
                        popup.status = Some("No session selected".to_string());
                        popup.phase = UploadPhase::Done;
                    }
                } else if let Some(ref mut popup) = app.upload_popup {
                    // All checked teams uploaded
                    popup.phase = UploadPhase::Done;
                    popup.status = None;
                }
            }
        }

        // Process any command generated above
        if let Some(cmd) = app.pending_command.take() {
            let result = rt.block_on(async_ops::execute(cmd, &app.daemon_config));
            app.apply_command_result(result);
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        // ── Deferred health check (runs once, after first render) ────
        if !app.health_check_done {
            app.health_check_done = true;
            if let Some(ref mut info) = app.server_info {
                if is_local_url(&info.url) {
                    let url = info.url.clone();
                    info.status = rt.block_on(check_health(&url));
                }
            }
        }

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

/// Try to store a session via platform API and return the body_url.
///
/// NOTE: This uses `reqwest::blocking` which must NOT be called inside
/// `tokio::Runtime::block_on()`. Currently safe because `try_git_store`
/// is called before the `rt.block_on(upload_session(...))` call.
fn try_git_store(
    session: &opensession_core::trace::Session,
    config: &config::DaemonConfig,
) -> Option<String> {
    if !matches!(
        config.git_storage.method,
        config::GitStorageMethod::PlatformApi
    ) {
        return None;
    }

    if config.git_storage.token.is_empty() {
        return None;
    }

    // Get working directory from session context
    let cwd = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str())?;

    // Extract remote URL via git CLI
    let git_ctx = opensession_local_db::git::extract_git_context(cwd);
    let remote_url = git_ctx.remote?;

    let jsonl = session.to_jsonl().ok()?;

    let storage = platform_api_storage::PlatformApiStorage::new(config.git_storage.token.clone());
    match storage.store(&remote_url, &session.session_id, jsonl.as_bytes()) {
        Ok(url) => Some(url),
        Err(e) => {
            eprintln!("git storage: {e}");
            None
        }
    }
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
