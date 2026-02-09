mod app;
mod ui;
mod views;

use anyhow::Result;
use app::{App, ServerInfo, ServerStatus};
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;
use serde::Deserialize;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Deserialize, Default)]
struct CliConfig {
    #[serde(default)]
    server: CliServerConfig,
}

#[derive(Deserialize, Default)]
struct CliServerConfig {
    #[serde(default = "default_server_url")]
    url: String,
}

fn default_server_url() -> String {
    "https://opensession.io".to_string()
}

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

    // Load server info from CLI config
    app.server_info = load_server_info();

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

fn load_server_info() -> Option<ServerInfo> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let config_path = PathBuf::from(home)
        .join(".config")
        .join("opensession")
        .join("config.toml");

    if !config_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: CliConfig = toml::from_str(&content).ok()?;

    if config.server.url.is_empty() {
        return None;
    }

    // Try to read last upload time from state.json
    let state_dir = PathBuf::from(
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_default(),
    )
    .join(".config")
    .join("opensession")
    .join("state.json");

    let last_upload = std::fs::read_to_string(&state_dir)
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
        url: config.server.url,
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
