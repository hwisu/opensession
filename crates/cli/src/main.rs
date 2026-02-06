mod config;
mod daemon_ctl;
mod discover;
mod upload;
mod upload_all;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "opensession", about = "opensession.io CLI - manage AI coding sessions")]
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
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the background daemon
    Start,
    /// Stop the background daemon
    Stop,
    /// Show daemon status
    Status,
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

    let result = match cli.command {
        Commands::Discover => discover::run_discover(),
        Commands::Upload { file } => upload::run_upload(&file).await,
        Commands::UploadAll => upload_all::run_upload_all().await,
        Commands::Config { server, api_key, team_id } => {
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
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
