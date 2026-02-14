mod app;
mod async_ops;
mod cli_export;
mod config;
mod platform_api_storage;
mod session_timeline;
mod theme;
mod timeline_summary;
mod ui;
mod views;

use anyhow::Result;
use app::{App, ServerInfo, ServerStatus, SetupStep, StartupStatus, UploadPhase, View};
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

pub use cli_export::{
    export_session_timeline, CliTimelineExport, CliTimelineExportOptions, CliTimelineView,
};

enum BgEvent {
    SessionsLoaded(Vec<opensession_core::trace::Session>),
    DbReady { repos: Vec<String>, count: usize },
}

#[derive(Debug, Clone, Default)]
pub struct SummaryLaunchOverride {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub content_mode: Option<String>,
    pub disk_cache_enabled: Option<bool>,
    pub openai_compat_endpoint: Option<String>,
    pub openai_compat_base: Option<String>,
    pub openai_compat_path: Option<String>,
    pub openai_compat_style: Option<String>,
    pub openai_compat_api_key: Option<String>,
    pub openai_compat_api_key_header: Option<String>,
}

impl SummaryLaunchOverride {
    pub fn has_any_override(&self) -> bool {
        self.provider.is_some()
            || self.model.is_some()
            || self.content_mode.is_some()
            || self.disk_cache_enabled.is_some()
            || self.openai_compat_endpoint.is_some()
            || self.openai_compat_base.is_some()
            || self.openai_compat_path.is_some()
            || self.openai_compat_style.is_some()
            || self.openai_compat_api_key.is_some()
            || self.openai_compat_api_key_header.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub paths: Option<Vec<String>>,
    pub auto_enter_detail: bool,
    pub summary_override: Option<SummaryLaunchOverride>,
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
    // Start with empty sessions — they'll load in the background
    let mut app = App::new(vec![]);
    app.loading_sessions = true;

    // ── Load full daemon config ──────────────────────────────────────
    let mut daemon_config = config::load_daemon_config();
    if let Some(summary_override) = options.summary_override.as_ref() {
        apply_summary_launch_override(&mut daemon_config, summary_override);
    }
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

    // ── Open main DB connection (fast — just opens SQLite file) ──────
    if let Ok(db) = LocalDb::open() {
        app.db = Some(Arc::new(db));
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
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    // ── Spawn background session loading thread ─────────────────────
    let (tx, bg_rx) = mpsc::channel::<BgEvent>();
    let paths = options.paths.clone();

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
    let result = event_loop(&mut terminal, &mut app, bg_rx, options.auto_enter_detail);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn apply_summary_launch_override(
    daemon_config: &mut config::DaemonConfig,
    summary_override: &SummaryLaunchOverride,
) {
    if let Some(provider) = summary_override.provider.clone() {
        daemon_config.daemon.summary_provider = Some(provider);
    }
    if let Some(model) = summary_override.model.clone() {
        daemon_config.daemon.summary_model = Some(model);
    }
    if let Some(mode) = summary_override.content_mode.clone() {
        daemon_config.daemon.summary_content_mode = mode;
    }
    if let Some(enabled) = summary_override.disk_cache_enabled {
        daemon_config.daemon.summary_disk_cache_enabled = enabled;
    }
    if let Some(endpoint) = summary_override.openai_compat_endpoint.clone() {
        daemon_config.daemon.summary_openai_compat_endpoint = Some(endpoint);
    }
    if let Some(base) = summary_override.openai_compat_base.clone() {
        daemon_config.daemon.summary_openai_compat_base = Some(base);
    }
    if let Some(path) = summary_override.openai_compat_path.clone() {
        daemon_config.daemon.summary_openai_compat_path = Some(path);
    }
    if let Some(style) = summary_override.openai_compat_style.clone() {
        daemon_config.daemon.summary_openai_compat_style = Some(style);
    }
    if let Some(key) = summary_override.openai_compat_api_key.clone() {
        daemon_config.daemon.summary_openai_compat_key = Some(key);
    }
    if let Some(key_header) = summary_override.openai_compat_api_key_header.clone() {
        daemon_config.daemon.summary_openai_compat_key_header = Some(key_header);
    }
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
        if App::is_internal_summary_session(session) {
            continue;
        }

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
    auto_enter_detail: bool,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let (summary_tx, summary_rx) = mpsc::channel::<async_ops::CommandResult>();
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
                    if !app.sessions.is_empty() {
                        app.list_state.select(Some(0));
                    }
                    app.loading_sessions = false;
                    if auto_enter_detail_pending && !app.sessions.is_empty() {
                        app.enter_detail_for_startup();
                        auto_enter_detail_pending = false;
                    }
                }
                BgEvent::DbReady { repos, count } => {
                    app.repos = repos;
                    app.startup_status.sessions_cached = count;
                    app.startup_status.repos_detected = app.repos.len();
                }
            }
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

        // ── Poll summary worker results ─────────────────────────────
        while let Ok(result) = summary_rx.try_recv() {
            app.apply_command_result(result);
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

        // ── Realtime detail preview (mtime polling + selective reparse) ──
        if let Some(path) = app.take_realtime_reload_path() {
            if let Some(reloaded) = parse_single_session(&path) {
                app.apply_reloaded_session(reloaded);
            }
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        // ── Timeline summary queue (non-stream sessions, visible-first) ──
        if let Some(cmd) = app.schedule_detail_summary_jobs() {
            spawn_summary_worker(cmd, app.daemon_config.clone(), summary_tx.clone());
        }

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

fn spawn_summary_worker(
    cmd: async_ops::AsyncCommand,
    config: config::DaemonConfig,
    tx: mpsc::Sender<async_ops::CommandResult>,
) {
    std::thread::spawn(move || {
        let result = match cmd {
            async_ops::AsyncCommand::GenerateTimelineSummary {
                key,
                epoch,
                provider,
                context,
                agent_tool,
            } => {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(rt) => rt.block_on(async_ops::execute(
                        async_ops::AsyncCommand::GenerateTimelineSummary {
                            key,
                            epoch,
                            provider,
                            context,
                            agent_tool,
                        },
                        &config,
                    )),
                    Err(err) => async_ops::CommandResult::SummaryDone {
                        key,
                        epoch,
                        result: Err(format!("failed to start summary runtime: {err}")),
                    },
                }
            }
            _ => return,
        };
        let _ = tx.send(result);
    });
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
                Ok(session) => {
                    if session.stats.event_count > 0 && !App::is_internal_summary_session(&session)
                    {
                        sessions.push(session);
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
                    if session.stats.event_count > 0 && !App::is_internal_summary_session(&session)
                    {
                        sessions.push(session);
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.context.created_at.cmp(&a.context.created_at));
    sessions
}

fn parse_single_session(path: &PathBuf) -> Option<opensession_core::trace::Session> {
    let parsers = opensession_parsers::all_parsers();
    let parser = parsers.iter().find(|p| p.can_parse(path))?;
    let session = parser.parse(path).ok()?;
    if session.stats.event_count == 0 || App::is_internal_summary_session(&session) {
        return None;
    }
    Some(session)
}

#[cfg(test)]
mod tests {
    use super::{apply_summary_launch_override, SummaryLaunchOverride};

    #[test]
    fn summary_override_updates_runtime_config_only() {
        let mut cfg = crate::config::DaemonConfig::default();
        let override_cfg = SummaryLaunchOverride {
            provider: Some("cli:codex".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            content_mode: Some("minimal".to_string()),
            disk_cache_enabled: Some(false),
            openai_compat_endpoint: Some("https://example.com/v1/chat/completions".to_string()),
            openai_compat_base: Some("https://example.com/v1".to_string()),
            openai_compat_path: Some("/chat/completions".to_string()),
            openai_compat_style: Some("chat".to_string()),
            openai_compat_api_key: Some("test-key".to_string()),
            openai_compat_api_key_header: Some("Authorization".to_string()),
        };

        apply_summary_launch_override(&mut cfg, &override_cfg);

        assert_eq!(cfg.daemon.summary_provider.as_deref(), Some("cli:codex"));
        assert_eq!(cfg.daemon.summary_model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(cfg.daemon.summary_content_mode, "minimal");
        assert!(!cfg.daemon.summary_disk_cache_enabled);
        assert_eq!(
            cfg.daemon.summary_openai_compat_endpoint.as_deref(),
            Some("https://example.com/v1/chat/completions")
        );
        assert_eq!(
            cfg.daemon.summary_openai_compat_base.as_deref(),
            Some("https://example.com/v1")
        );
        assert_eq!(
            cfg.daemon.summary_openai_compat_path.as_deref(),
            Some("/chat/completions")
        );
        assert_eq!(
            cfg.daemon.summary_openai_compat_style.as_deref(),
            Some("chat")
        );
        assert_eq!(
            cfg.daemon.summary_openai_compat_key.as_deref(),
            Some("test-key")
        );
        assert_eq!(
            cfg.daemon.summary_openai_compat_key_header.as_deref(),
            Some("Authorization")
        );
    }
}
