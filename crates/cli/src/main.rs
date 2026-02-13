mod config;
mod daemon_ctl;
mod discover;
mod handoff;
mod index;
mod log_cmd;
mod output;
pub mod server;
mod session_ref;
mod stats;
mod stream_push;
mod summarize;
#[cfg(feature = "e2e")]
mod test_cmd;
mod upload;
mod upload_all;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::output::OutputFormat;

/// Time period for stats aggregation.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum StatsPeriod {
    Day,
    Week,
    Month,
    All,
}

/// Structured exit codes (gh CLI pattern).
#[repr(u8)]
#[derive(PartialEq)]
#[allow(dead_code)]
enum ExitCode {
    Success = 0,
    GeneralError = 1,
    UsageError = 2,
    NoData = 3,
    AuthError = 4,
    NetworkError = 5,
}

impl ExitCode {
    /// Classify an anyhow::Error into the appropriate exit code.
    fn from_error(err: &anyhow::Error) -> Self {
        let msg = format!("{err:#}").to_lowercase();

        if msg.contains("api key")
            || msg.contains("api_key")
            || msg.contains("auth")
            || msg.contains("unauthorized")
            || msg.contains("forbidden")
            || msg.contains("osk_")
        {
            return ExitCode::AuthError;
        }

        if msg.contains("network")
            || msg.contains("connection")
            || msg.contains("timeout")
            || msg.contains("dns")
            || msg.contains("failed to call")
            || msg.contains("reqwest")
        {
            return ExitCode::NetworkError;
        }

        if msg.contains("no session")
            || msg.contains("not found")
            || msg.contains("no data")
            || msg.contains("empty")
        {
            return ExitCode::NoData;
        }

        if msg.contains("usage") || msg.contains("invalid") || msg.contains("parse") {
            return ExitCode::UsageError;
        }

        ExitCode::GeneralError
    }
}

#[derive(Parser)]
#[command(
    name = "opensession",
    about = "opensession.io CLI - manage AI coding sessions"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all local AI sessions found on this machine
    Discover,

    /// Upload a session file to the server (or git branch with --git)
    Upload {
        /// Path to the session file
        file: PathBuf,

        /// Link to parent session(s) by ID (can be specified multiple times)
        #[arg(long)]
        parent: Vec<String>,

        /// Store to git branch (opensession/sessions) instead of server
        #[arg(long)]
        git: bool,
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

    /// Run E2E tests against a server (requires --features e2e)
    #[cfg(feature = "e2e")]
    Test(test_cmd::TestArgs),

    /// Generate a session handoff summary for the next agent
    Handoff {
        /// Session file(s). Multiple files can be specified for merged handoff
        files: Vec<PathBuf>,

        /// Use the most recent session
        #[arg(short, long)]
        last: bool,

        /// Write output to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Output format
        #[arg(long, value_enum, default_value = "markdown")]
        format: OutputFormat,

        /// Generate additional LLM-powered summary (requires API key)
        #[arg(long)]
        summarize: bool,

        /// Claude Code session reference (e.g. HEAD, HEAD~2)
        #[arg(long)]
        claude: Option<String>,

        /// Gemini session reference (e.g. HEAD, HEAD~1)
        #[arg(long)]
        gemini: Option<String>,

        /// Generic tool session reference (e.g. "amp HEAD~2")
        #[arg(long)]
        tool: Vec<String>,

        /// AI provider for summarization: "claude", "openai", "gemini"
        #[arg(long)]
        ai: Option<String>,
    },

    /// Build/update the local session index from discovered session files
    Index,

    /// Show session history (git-log style)
    Log {
        /// Show sessions from the last N hours/days (e.g. "3 hours", "2 days", "1 week")
        #[arg(long)]
        since: Option<String>,

        /// Show sessions before this time
        #[arg(long)]
        before: Option<String>,

        /// Filter by tool name (e.g. "claude-code", "gemini")
        #[arg(long)]
        tool: Option<String>,

        /// Filter by model (supports * wildcards, e.g. "opus*")
        #[arg(long)]
        model: Option<String>,

        /// Show sessions that touched a specific file
        #[arg(long)]
        touches: Option<String>,

        /// Search in session titles and descriptions
        #[arg(long)]
        grep: Option<String>,

        /// Show only sessions with errors
        #[arg(long)]
        has_errors: bool,

        /// Filter by working directory / project path
        #[arg(long)]
        project: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,

        /// Maximum number of results
        #[arg(short = 'n', long, default_value = "20")]
        limit: u32,

        /// Select specific JSON fields (e.g. "id,tool,title"). Use without value to list available fields
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        json: Option<String>,

        /// Apply a jq filter to JSON output (built-in: ., .field, .[], .[].field, .[N], length, keys; complex: system jq)
        #[arg(long)]
        jq: Option<String>,
    },

    /// Show AI session usage statistics
    Stats {
        /// Time period
        #[arg(long, value_enum, default_value = "week")]
        period: StatsPeriod,

        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        format: OutputFormat,
    },

    /// Compare two sessions side-by-side
    Diff {
        /// First session (ID, file path, or reference like HEAD^2)
        session_a: String,

        /// Second session (ID, file path, or reference like HEAD^1)
        session_b: String,

        /// Use AI to analyze differences
        #[arg(long)]
        ai: bool,
    },

    /// Manage git hooks integration
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },

    /// Generate shell completions
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Real-time session streaming
    Stream {
        #[command(subcommand)]
        action: StreamAction,
    },

    /// Stream new events from a local session file (called by hooks)
    StreamPush {
        /// Agent name (e.g. "claude-code")
        #[arg(long)]
        agent: String,
    },
}

impl Commands {
    /// Whether this subcommand wants JSON-formatted error output.
    fn wants_json_errors(&self) -> bool {
        match self {
            Commands::Log { format, json, .. } => {
                matches!(format, OutputFormat::Json | OutputFormat::Stream) || json.is_some()
            }
            Commands::Handoff { format, .. } => {
                matches!(
                    format,
                    OutputFormat::Json | OutputFormat::Stream | OutputFormat::Jsonl
                )
            }
            Commands::Stats { format, .. } => matches!(format, OutputFormat::Json),
            _ => false,
        }
    }
}

fn suggestion_for_code(code: &ExitCode) -> Option<&'static str> {
    match code {
        ExitCode::AuthError => Some("opensession config --api-key <key>"),
        ExitCode::NoData => Some("opensession index"),
        ExitCode::NetworkError => Some("opensession server status"),
        ExitCode::UsageError => Some("opensession --help"),
        _ => None,
    }
}

#[derive(Subcommand)]
enum HooksAction {
    /// Install the prepare-commit-msg hook
    Install,
    /// Remove the prepare-commit-msg hook
    Uninstall,
}

#[derive(Subcommand)]
enum StreamAction {
    /// Enable real-time session streaming
    Enable {
        /// Agent name (auto-detected if omitted)
        #[arg(long)]
        agent: Option<String>,
    },
    /// Disable real-time session streaming
    Disable {
        /// Agent name (auto-detected if omitted)
        #[arg(long)]
        agent: Option<String>,
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

    // No subcommand → launch TUI
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            if let Err(e) = opensession_tui::run(None) {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
            return;
        }
    };

    // Auto-start daemon for commands that benefit from it
    match &command {
        Commands::Discover | Commands::Upload { .. } | Commands::UploadAll => {
            maybe_auto_start_daemon();
        }
        _ => {}
    }

    let json_errors = command.wants_json_errors();

    let result = match command {
        #[cfg(feature = "e2e")]
        Commands::Test(args) => test_cmd::run_test(args).await,
        Commands::Discover => discover::run_discover(),
        Commands::Upload { file, parent, git } => upload::run_upload(&file, &parent, git).await,
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
        Commands::Handoff {
            files,
            last,
            output,
            format,
            summarize,
            claude,
            gemini,
            tool,
            ai,
        } => {
            handoff::run_handoff(
                &files,
                last,
                output.as_deref(),
                format,
                summarize,
                claude.as_deref(),
                gemini.as_deref(),
                &tool,
                ai.as_deref(),
            )
            .await
        }
        Commands::Index => index::run_index(),
        Commands::Log {
            since,
            before,
            tool,
            model,
            touches,
            grep,
            has_errors,
            project,
            format,
            limit,
            json,
            jq,
        } => log_cmd::run_log(
            since.as_deref(),
            before.as_deref(),
            tool.as_deref(),
            model.as_deref(),
            touches.as_deref(),
            grep.as_deref(),
            has_errors,
            project.as_deref(),
            &format,
            limit,
            json.as_deref(),
            jq.as_deref(),
        ),
        Commands::Stats { period, format } => stats::run_stats(period, &format),
        Commands::Diff {
            session_a,
            session_b,
            ai,
        } => handoff::run_diff(&session_a, &session_b, ai).await,
        Commands::Hooks { action } => match action {
            HooksAction::Install => handoff::run_hooks_install(),
            HooksAction::Uninstall => handoff::run_hooks_uninstall(),
        },
        Commands::Stream { action } => run_stream_action(action),
        Commands::Completion { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
            Ok(())
        }
        Commands::StreamPush { agent } => stream_push::run_stream_push(&agent),
    };

    if let Err(e) = result {
        let code = ExitCode::from_error(&e);
        if json_errors {
            let diag = output::CliDiagnostic::error(
                &format!("{e}"),
                &format!("{e:#}"),
                suggestion_for_code(&code),
            );
            println!("{}", serde_json::to_string(&diag).unwrap_or_default());
        } else {
            eprintln!("Error: {:#}", e);
        }
        std::process::exit(code as i32);
    }
}

/// Run stream enable/disable with auto-detection when `--agent` is omitted.
fn run_stream_action(action: StreamAction) -> anyhow::Result<()> {
    let (agent_arg, is_enable) = match &action {
        StreamAction::Enable { agent } => (agent.clone(), true),
        StreamAction::Disable { agent } => (agent.clone(), false),
    };

    let agents = if let Some(agent) = agent_arg {
        vec![agent]
    } else {
        // Auto-detect: discover tools with sessions on this machine
        let locations = opensession_parsers::discover::discover_sessions();
        let available: Vec<String> = locations
            .into_iter()
            .filter(|loc| !loc.paths.is_empty())
            .map(|loc| loc.tool)
            .collect();

        match available.len() {
            0 => anyhow::bail!("No AI sessions found. Use --agent to specify manually."),
            1 => {
                println!("Auto-detected: {}", available[0]);
                available
            }
            _ => {
                let selections = dialoguer::MultiSelect::new()
                    .with_prompt("Select agents to stream")
                    .items(&available)
                    .interact()?;

                if selections.is_empty() {
                    anyhow::bail!("No agents selected.");
                }

                selections
                    .into_iter()
                    .map(|i| available[i].clone())
                    .collect()
            }
        }
    };

    for agent in &agents {
        if is_enable {
            stream_push::enable_stream_write(agent)?;
        } else {
            stream_push::disable_stream_write(agent)?;
        }
    }

    Ok(())
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
