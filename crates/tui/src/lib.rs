mod app;
mod async_ops;
mod config;
mod live;
mod session_timeline;
mod theme;
mod timeline_summary;
mod ui;
mod views;

use anyhow::Result;
use app::{App, ServerInfo, ServerStatus, SetupStep, StartupStatus, UploadPhase, View};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use opensession_core::trace::{Agent, Session, SessionContext, Stats};
use opensession_local_db::git::extract_git_context;
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionRow};
use ratatui::prelude::*;
use std::collections::HashMap;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

enum BgEvent {
    SessionsLoaded(Vec<opensession_core::trace::Session>),
    DbReady { repos: Vec<String>, count: usize },
}

#[derive(Clone)]
struct LoadedSession {
    source_path: PathBuf,
    session: opensession_core::trace::Session,
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub paths: Option<Vec<String>>,
    pub auto_enter_detail: bool,
    pub focus_detail_view: bool,
}

/// Launch the TUI. Optionally pass file paths to open specific sessions.
pub fn run(paths: Option<Vec<String>>) -> Result<()> {
    run_with_options(RunOptions {
        paths,
        ..RunOptions::default()
    })
}

/// Launch the TUI with startup/runtime overrides.
pub fn run_with_options(options: RunOptions) -> Result<()> {
    run_with_options_sync(options)
}

fn run_with_options_sync(options: RunOptions) -> Result<()> {
    let mouse_capture_enabled = env_flag_enabled("OPS_TUI_MOUSE_CAPTURE");
    // Start with empty sessions — they'll load in the background
    let mut app = App::new(vec![]);
    app.loading_sessions = true;

    // ── Load full daemon config ──────────────────────────────────────
    let daemon_config = config::load_daemon_config();
    let config_exists = config::config_dir()
        .map(|d| d.join("opensession.toml").exists())
        .unwrap_or(false);

    // Build server info from daemon config
    app.server_info = build_server_info(&daemon_config);
    app.team_id = if daemon_config.identity.team_id.is_empty() {
        None
    } else {
        Some(daemon_config.identity.team_id.clone())
    };
    app.daemon_config = daemon_config;
    app.realtime_preview_enabled = app.daemon_config.daemon.detail_realtime_preview_enabled;
    app.connection_ctx = App::derive_connection_ctx(&app.daemon_config);
    app.focus_detail_view = options.focus_detail_view;

    // ── Build startup status ─────────────────────────────────────────
    let status = StartupStatus {
        sessions_cached: 0,
        repos_detected: 0,
        daemon_pid: config::daemon_pid(),
        config_exists,
    };
    app.startup_status = status;
    app.sync_daemon_publish_policy_from_runtime();

    // ── Open main DB connection (fast — just opens SQLite file) ──────
    if let Ok(db) = LocalDb::open() {
        app.db = Some(Arc::new(db));
    }

    // ── Fast bootstrap from local SQLite cache (avoid full repo scan on every start) ──
    if options.paths.is_none() {
        if let Some(db) = app.db.clone() {
            app.sessions = load_cached_sessions_from_db(&db);
            app.rebuild_session_agent_metrics();
            app.filtered_sessions = (0..app.sessions.len()).collect();
            app.rebuild_available_tools();
            if !app.sessions.is_empty() {
                app.list_state.select(Some(0));
            }
            app.repos = db.list_repos().unwrap_or_default();
            app.startup_status.sessions_cached = app.sessions.len();
            app.startup_status.repos_detected = app.repos.len();
            app.loading_sessions = app.sessions.is_empty();
        }
    }

    // ── If config file doesn't exist yet, start in Setup view ──────
    if !config_exists && !options.auto_enter_detail {
        app.view = View::Setup;
        app.setup_step = SetupStep::Scenario;
        app.setup_scenario_index = 0;
        app.settings_index = 0;
    }

    // Terminal setup — show UI immediately
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    if mouse_capture_enabled {
        stdout().execute(EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // ── Spawn background session loading thread ─────────────────────
    let (tx, bg_rx) = mpsc::channel::<BgEvent>();
    let paths = options.paths.clone();
    let should_refresh_from_disk =
        paths.is_some() || app.sessions.is_empty() || refresh_discovery_on_start();
    if should_refresh_from_disk {
        std::thread::spawn(move || {
            let sessions = match paths {
                Some(ref paths) => load_from_paths(paths),
                None => load_sessions(),
            };
            let sessions_for_ui = if paths.is_none() {
                filter_visible_discovered_sessions(
                    sessions.iter().map(|entry| entry.session.clone()).collect(),
                )
            } else {
                sessions.iter().map(|entry| entry.session.clone()).collect()
            };
            let ui_sessions = sessions_for_ui;
            let for_db = sessions.clone();
            if tx.send(BgEvent::SessionsLoaded(ui_sessions)).is_err() {
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
    }

    // Main loop
    let result = event_loop(
        &mut terminal,
        &mut app,
        bg_rx,
        options.auto_enter_detail,
        mouse_capture_enabled,
    );

    // Restore terminal
    disable_raw_mode()?;
    if mouse_capture_enabled {
        stdout().execute(DisableMouseCapture)?;
    }
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

    // Try to read last upload time from state.json fallback.
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

fn parse_cached_datetime(value: &str) -> chrono::DateTime<chrono::Utc> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&chrono::Utc);
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return dt.and_utc();
    }
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::UNIX_EPOCH)
}

fn parse_cached_tags(tags: Option<&str>) -> Vec<String> {
    let Some(raw) = tags.map(str::trim).filter(|v| !v.is_empty()) else {
        return Vec::new();
    };
    if let Ok(json_tags) = serde_json::from_str::<Vec<String>>(raw) {
        return json_tags
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect();
    }
    raw.split_whitespace()
        .map(|tag| tag.trim_start_matches('#').to_string())
        .filter(|tag| !tag.is_empty())
        .collect()
}

fn session_from_cached_row(row: &LocalSessionRow) -> Session {
    let created_at = parse_cached_datetime(&row.created_at);
    let mut session = Session::new(
        row.id.clone(),
        Agent {
            provider: row
                .agent_provider
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("unknown")
                .to_string(),
            model: row
                .agent_model
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("unknown")
                .to_string(),
            tool: row.tool.clone(),
            tool_version: None,
        },
    );

    let mut attributes: HashMap<String, serde_json::Value> = HashMap::new();
    if let Some(source_path) = row.source_path.as_deref().filter(|v| !v.trim().is_empty()) {
        attributes.insert(
            "source_path".to_string(),
            serde_json::Value::String(source_path.to_string()),
        );
    }
    if let Some(working_directory) = row
        .working_directory
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        attributes.insert(
            "working_directory".to_string(),
            serde_json::Value::String(working_directory.to_string()),
        );
    }
    if let Some(nickname) = row.nickname.as_deref().filter(|v| !v.trim().is_empty()) {
        attributes.insert(
            "nickname".to_string(),
            serde_json::Value::String(nickname.to_string()),
        );
    }
    if let Some(user_id) = row.user_id.as_deref().filter(|v| !v.trim().is_empty()) {
        attributes.insert(
            "user_id".to_string(),
            serde_json::Value::String(user_id.to_string()),
        );
    }
    if let Some(team_id) = row.team_id.as_deref().filter(|v| !v.trim().is_empty()) {
        attributes.insert(
            "team_id".to_string(),
            serde_json::Value::String(team_id.to_string()),
        );
    }
    if let Some(git_repo) = row
        .git_repo_name
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        attributes.insert(
            "git_repo_name".to_string(),
            serde_json::Value::String(git_repo.to_string()),
        );
    }
    if let Some(git_branch) = row.git_branch.as_deref().filter(|v| !v.trim().is_empty()) {
        attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String(git_branch.to_string()),
        );
    }

    session.context = SessionContext {
        title: row.title.clone(),
        description: row.description.clone(),
        tags: parse_cached_tags(row.tags.as_deref()),
        created_at,
        updated_at: created_at,
        related_session_ids: Vec::new(),
        attributes,
    };
    session.stats = Stats {
        event_count: row.event_count.max(0) as u64,
        message_count: row.message_count.max(0) as u64,
        tool_call_count: 0,
        task_count: row.task_count.max(0) as u64,
        duration_seconds: row.duration_seconds.max(0) as u64,
        total_input_tokens: row.total_input_tokens.max(0) as u64,
        total_output_tokens: row.total_output_tokens.max(0) as u64,
        user_message_count: row.user_message_count.max(0) as u64,
        files_changed: 0,
        lines_added: 0,
        lines_removed: 0,
    };
    session
}

fn load_cached_sessions_from_db(db: &LocalDb) -> Vec<Session> {
    let rows = db
        .list_sessions(&LocalSessionFilter::default())
        .unwrap_or_default();
    let mut sessions: Vec<Session> = rows
        .iter()
        .map(session_from_cached_row)
        .filter(|session| !App::is_internal_summary_session(session))
        .collect();
    sessions.sort_by(|a, b| b.context.created_at.cmp(&a.context.created_at));
    sessions
}

fn refresh_discovery_on_start() -> bool {
    let Ok(raw) = std::env::var("OPS_TUI_REFRESH_DISCOVERY_ON_START") else {
        return true;
    };
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
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
/// Existing rows are backfilled with git/cwd metadata when parsed `cwd` is available.
fn cache_sessions_to_db(db: &LocalDb, sessions: &[LoadedSession]) {
    let existing = db.existing_session_ids();

    for item in sessions {
        let session = &item.session;
        let source = item.source_path.to_string_lossy();
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str().map(String::from));

        if App::is_internal_summary_session(session) {
            continue;
        }

        if existing.contains(&session.session_id) {
            if cwd.is_some() {
                // Existing rows can miss repo/cwd metadata. When cwd exists in parsed data,
                // re-upsert to backfill git context as well as stats.
                let git = cwd.as_deref().map(extract_git_context).unwrap_or_default();
                let _ = db.upsert_local_session(session, &source, &git);
            } else {
                // Legacy fallback when no cwd is present in parsed data.
                let _ = db.update_session_stats(session);
                let _ = db.set_session_sync_path(&session.session_id, &source);
            }
            continue;
        }

        // New session → full insert with git context extraction
        let git = cwd.as_deref().map(extract_git_context).unwrap_or_default();

        let _ = db.upsert_local_session(session, &source, &git);
    }
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    bg_rx: mpsc::Receiver<BgEvent>,
    auto_enter_detail: bool,
    mouse_capture_enabled: bool,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let mut auto_enter_detail_pending = auto_enter_detail;

    loop {
        // ── Poll background session loading ──────────────────────────
        while let Ok(ev) = bg_rx.try_recv() {
            match ev {
                BgEvent::SessionsLoaded(sessions) => {
                    app.sessions = sessions
                        .into_iter()
                        .filter(|session| !App::is_internal_summary_session(session))
                        .collect();
                    app.rebuild_session_agent_metrics();
                    app.filtered_sessions = (0..app.sessions.len()).collect();
                    app.rebuild_available_tools();
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

        if auto_enter_detail_pending && !app.loading_sessions && !app.sessions.is_empty() {
            app.enter_detail_for_startup();
            auto_enter_detail_pending = false;
        }

        if app.focus_detail_view
            && !matches!(app.view, View::SessionDetail | View::Help | View::Setup)
            && !app.loading_sessions
            && !app.sessions.is_empty()
        {
            if app.list_state.selected().is_none() {
                app.list_state.select(Some(0));
            }
            app.enter_detail_for_startup();
        }

        // ── Handle pending async command ─────────────────────────────
        if let Some(cmd) = app.pending_command.take() {
            let result = rt.block_on(async_ops::execute(cmd, &app.daemon_config));
            app.apply_command_result(result);
        }

        // ── Handle upload popup async ops ────────────────────────────
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

        // ── Lazy hydrate stub sessions from source_path on first detail enter ──
        if let Some(path) = app.take_detail_hydrate_path() {
            match parse_single_session(&path) {
                Ok(reloaded) => {
                    app.apply_reloaded_session(reloaded);
                }
                Err(err) => {
                    let message = format!("Hydration skipped: {err}");
                    app.record_selected_session_detail_issue(message.clone());
                    app.flash_error(message);
                }
            }
        }

        // ── Realtime detail preview (live provider + tail-follow aware update) ──
        if let Some(batch) = app.poll_live_update_batch() {
            app.apply_live_update_batch(batch);
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
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if app.handle_key(key.code) {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    if !mouse_capture_enabled {
                        continue;
                    }
                    if app.handle_mouse(mouse) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Try to store a session in git-native branch storage and return the body_url.
/// SQLite mode skips branch storage and falls back to server-side body storage.
fn try_git_store(
    session: &opensession_core::trace::Session,
    config: &config::DaemonConfig,
) -> Option<String> {
    if matches!(config.git_storage.method, config::GitStorageMethod::Sqlite) {
        return None;
    }

    // Get working directory from session context.
    let cwd = session
        .context
        .attributes
        .get("cwd")
        .or_else(|| session.context.attributes.get("working_directory"))
        .and_then(|v| v.as_str())?;

    let repo_root = opensession_git_native::ops::find_repo_root(Path::new(cwd))?;
    let git_ctx = opensession_local_db::git::extract_git_context(cwd);
    let remote_url = git_ctx.remote?;

    let jsonl = session.to_jsonl().ok()?;
    let storage = opensession_git_native::NativeGitStorage;
    match storage.store(&repo_root, &session.session_id, jsonl.as_bytes(), b"{}") {
        Ok(rel_path) => Some(opensession_git_native::generate_raw_url(
            &remote_url,
            &rel_path,
        )),
        Err(e) => {
            eprintln!("git-native storage: {e}");
            None
        }
    }
}

/// Load sessions from explicit file paths passed as CLI args.
fn load_from_paths(args: &[String]) -> Vec<LoadedSession> {
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
                Ok(session) => {
                    if session.stats.event_count > 0 && !App::is_internal_summary_session(&session)
                    {
                        sessions.push(LoadedSession {
                            source_path: path.clone(),
                            session,
                        });
                    } else if session.stats.event_count == 0 {
                        eprintln!(
                            "Warning: skipping empty session from {} ({})",
                            path.display(),
                            parser.name()
                        );
                    }
                }
                Err(e) => eprintln!("Warning: failed to parse {}: {}", path.display(), e),
            }
        } else {
            eprintln!("Warning: no parser for {}", path.display());
        }
    }

    sessions.sort_by(|a, b| {
        b.session
            .context
            .created_at
            .cmp(&a.session.context.created_at)
    });
    sessions
}

fn is_hidden_opencode_child_session(session: &opensession_core::trace::Session) -> bool {
    if session.agent.tool != "opencode" {
        return false;
    }

    if !session.context.related_session_ids.is_empty() {
        return true;
    }

    if session
        .context
        .attributes
        .iter()
        .any(|(key, value)| opencode_parent_session_id(value, key))
    {
        return true;
    }

    let session_id = session.session_id.to_ascii_lowercase();
    if session_id.starts_with("agent-") || session_id.starts_with("agent_") {
        return true;
    }

    if session.stats.user_message_count == 0
        && session.stats.message_count <= 4
        && session.stats.task_count <= 4
        && session.stats.event_count > 0
        && session.stats.event_count <= 16
    {
        return true;
    }

    if let Some(path) = session.context.attributes.get("source_path") {
        let path = path.as_str().unwrap_or_default().to_ascii_lowercase();
        if path.contains("/subagents/") || path.contains("\\subagents\\") {
            return true;
        }
    }

    false
}

fn opencode_parent_session_id(value: &serde_json::Value, key: &str) -> bool {
    if let Some(parent_id) = value.as_str() {
        let compact_key = key
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if !parent_id.trim().is_empty() {
            return matches!(
                compact_key.as_str(),
                "parentsessionid" | "parentid" | "parentuuid"
            );
        }
    }
    false
}

fn is_hidden_claude_code_child_session(session: &opensession_core::trace::Session) -> bool {
    if session.agent.tool != "claude-code" {
        return false;
    }

    if !session.context.related_session_ids.is_empty() {
        return true;
    }

    let Some(path) = session.context.attributes.get("source_path") else {
        return false;
    };
    let Some(path) = path.as_str() else {
        return false;
    };

    opensession_parsers::claude_code::is_claude_subagent_path(std::path::Path::new(path))
}

fn filter_visible_discovered_sessions(
    sessions: Vec<opensession_core::trace::Session>,
) -> Vec<opensession_core::trace::Session> {
    sessions
        .into_iter()
        .filter(|session| {
            !is_hidden_opencode_child_session(session)
                && !is_hidden_claude_code_child_session(session)
        })
        .collect()
}

/// Auto-discover sessions from known local paths.
fn load_sessions() -> Vec<LoadedSession> {
    let locations = opensession_parsers::discover::discover_sessions();
    let parsers = opensession_parsers::all_parsers();
    let mut sessions = Vec::new();

    for location in &locations {
        for path in &location.paths {
            // Skip subagent session files
            if opensession_parsers::claude_code::is_claude_subagent_path(path) {
                continue;
            }

            if let Some(parser) = parsers.iter().find(|p| p.can_parse(path)) {
                if let Ok(session) = parser.parse(path) {
                    // Skip empty sessions (0 events usually means parse was incomplete)
                    if session.stats.event_count > 0 && !App::is_internal_summary_session(&session)
                    {
                        sessions.push(LoadedSession {
                            source_path: path.clone(),
                            session,
                        });
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| {
        b.session
            .context
            .created_at
            .cmp(&a.session.context.created_at)
    });
    sessions
}

fn parse_single_session(path: &Path) -> Result<opensession_core::trace::Session, String> {
    let parsers = opensession_parsers::all_parsers();
    let Some(parser) = parsers.iter().find(|p| p.can_parse(path)) else {
        return Err(format!("no parser matched {}", path.display()));
    };
    let session = parser
        .parse(path)
        .map_err(|err| format!("parse failed ({}): {err}", parser.name()))?;
    if session.stats.event_count == 0 || App::is_internal_summary_session(&session) {
        if let Some(hint) = App::source_error_hint(path) {
            return Err(format!(
                "parsed as 0 events ({}), detected source error: {hint}",
                parser.name()
            ));
        }
        return Err(format!("parsed as 0 events ({})", parser.name()));
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::{
        env_flag_enabled, filter_visible_discovered_sessions, is_hidden_opencode_child_session,
        refresh_discovery_on_start,
    };
    use chrono::Utc;
    use opensession_core::trace::{Agent, Session, SessionContext};
    use serde_json::json;

    fn make_opencode_session(session_id: &str, related_session_ids: Vec<&str>) -> Session {
        let mut session = Session::new(
            session_id.to_string(),
            Agent {
                provider: "provider".to_string(),
                model: "model".to_string(),
                tool: "opencode".to_string(),
                tool_version: None,
            },
        );
        session.context = SessionContext {
            created_at: Utc::now(),
            updated_at: Utc::now(),
            related_session_ids: related_session_ids
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            ..SessionContext::default()
        };
        session
    }

    fn make_codex_session(session_id: &str, tool: &str, user_message_count: u64) -> Session {
        let mut session = Session::new(
            session_id.to_string(),
            Agent {
                provider: "provider".to_string(),
                model: "model".to_string(),
                tool: tool.to_string(),
                tool_version: None,
            },
        );
        session.stats.user_message_count = user_message_count;
        session
    }

    #[test]
    fn opencode_child_session_is_hidden_in_discovery_list() {
        let child = make_opencode_session("ses_child", vec!["ses_parent"]);
        let parent = make_opencode_session("ses_parent", vec![]);
        assert!(is_hidden_opencode_child_session(&child));
        assert!(!is_hidden_opencode_child_session(&parent));
    }

    #[test]
    fn opencode_zero_user_message_short_session_is_hidden_in_discovery_list() {
        let mut child = make_opencode_session("ses_short", vec![]);
        child.stats.user_message_count = 0;
        child.stats.event_count = 4;
        child.stats.message_count = 0;
        child.stats.task_count = 0;

        let mut parent = make_opencode_session("ses_visible", vec![]);
        parent.stats.user_message_count = 2;
        parent.stats.message_count = 3;
        parent.stats.event_count = 40;

        assert!(is_hidden_opencode_child_session(&child));
        assert!(!is_hidden_opencode_child_session(&parent));
    }

    #[test]
    fn opencode_session_with_parent_id_attr_alias_is_hidden_in_discovery_list() {
        let mut child = make_opencode_session("ses_short", vec![]);
        child
            .context
            .attributes
            .insert("parentSessionId".to_string(), json!("ses_parent_alias"));
        assert!(is_hidden_opencode_child_session(&child));
    }

    #[test]
    fn codex_session_without_user_message_is_not_hidden_in_discovery_list() {
        let summary_session = make_codex_session("summary", "codex", 0);
        let normal_session = make_codex_session("normal", "codex", 1);
        let visible = filter_visible_discovered_sessions(vec![
            summary_session.clone(),
            normal_session.clone(),
        ]);

        assert_eq!(visible.len(), 2);
        assert!(visible
            .iter()
            .any(|session| session.session_id == "summary"));
        assert!(visible.iter().any(|session| session.session_id == "normal"));
    }

    #[test]
    fn env_flag_enabled_defaults_false() {
        let key = "OPS_TUI_FLAG_TEST_FALSE";
        std::env::remove_var(key);
        assert!(!env_flag_enabled(key));
    }

    #[test]
    fn env_flag_enabled_accepts_true_values() {
        let key = "OPS_TUI_FLAG_TEST_TRUE";
        std::env::set_var(key, "true");
        assert!(env_flag_enabled(key));
        std::env::set_var(key, "1");
        assert!(env_flag_enabled(key));
        std::env::remove_var(key);
    }

    #[test]
    fn refresh_discovery_on_start_defaults_true() {
        let key = "OPS_TUI_REFRESH_DISCOVERY_ON_START";
        std::env::remove_var(key);
        assert!(refresh_discovery_on_start());
    }

    #[test]
    fn refresh_discovery_on_start_accepts_false_values() {
        let key = "OPS_TUI_REFRESH_DISCOVERY_ON_START";
        std::env::set_var(key, "off");
        assert!(!refresh_discovery_on_start());
        std::env::set_var(key, "0");
        assert!(!refresh_discovery_on_start());
        std::env::remove_var(key);
    }
}
