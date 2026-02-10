mod config;
mod daemon_ctl;
mod discover;
mod handoff;
pub mod server;
mod upload;
mod upload_all;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "opensession",
    about = "opensession.io CLI - manage AI coding sessions"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all local AI sessions found on this machine
    Discover,

    /// Upload a session file to the server
    Upload {
        /// Path to the session file
        file: PathBuf,
    },

    /// Discover and upload ALL local sessions to the server
    UploadAll,

    /// Show or set configuration
    Config {
        /// Set the server URL
        #[arg(long)]
        server: Option<String>,

        /// Set the API key
        #[arg(long)]
        api_key: Option<String>,

        /// Set the team ID for uploads
        #[arg(long)]
        team_id: Option<String>,
    },

    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Check server connection and authentication
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },

    /// Generate a session handoff summary for the next agent
    Handoff {
        /// Path to the session file (omit to select interactively)
        file: Option<PathBuf>,

        /// Use the most recent session
        #[arg(short, long)]
        last: bool,

        /// Write output to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the background daemon
    Start,
    /// Stop the background daemon
    Stop,
    /// Show daemon status
    Status,
    /// Check daemon and server health
    Health,
}

#[derive(Subcommand)]
enum ServerAction {
    /// Check server health and version
    Status,
    /// Verify API key authentication
    Verify,
}

/// Auto-start daemon if configured and not already running
fn maybe_auto_start_daemon() {
    let cfg = match config::load_config() {
        Ok(c) => c,
        Err(_) => return,
    };

    if cfg.daemon.auto_start && !daemon_ctl::is_daemon_running() {
        eprintln!("Auto-starting daemon...");
        if let Err(e) = daemon_ctl::daemon_start() {
            eprintln!("Warning: failed to auto-start daemon: {}", e);
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cli = Cli::parse();

    // Auto-start daemon for commands that benefit from it
    match &cli.command {
        Commands::Discover | Commands::Upload { .. } | Commands::UploadAll => {
            maybe_auto_start_daemon();
        }
        _ => {}
    }

    let result = match cli.command {
        Commands::Discover => discover::run_discover(),
        Commands::Upload { file } => upload::run_upload(&file).await,
        Commands::UploadAll => upload_all::run_upload_all().await,
        Commands::Config {
            server,
            api_key,
            team_id,
        } => {
            if server.is_none() && api_key.is_none() && team_id.is_none() {
                config::show_config()
            } else {
                config::set_config(server, api_key, team_id)
            }
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start => daemon_ctl::daemon_start(),
            DaemonAction::Stop => daemon_ctl::daemon_stop(),
            DaemonAction::Status => daemon_ctl::daemon_status(),
            DaemonAction::Health => run_daemon_health().await,
        },
        Commands::Server { action } => match action {
            ServerAction::Status => server::run_status().await,
            ServerAction::Verify => server::run_verify().await,
        },
        Commands::Handoff { file, last, output } => {
            handoff::run_handoff(file.as_deref(), last, output.as_deref())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

/// Run daemon health check from CLI
async fn run_daemon_health() -> anyhow::Result<()> {
    // Check daemon status
    if daemon_ctl::is_daemon_running() {
        println!("Daemon: running");
    } else {
        println!("Daemon: not running");
    }

    // Check server
    server::run_status().await?;

    Ok(())
}
