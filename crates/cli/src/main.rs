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
mod tui_cmd;
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
    /// Launch the interactive terminal UI
    Ui,

    /// Session workflows (discover/history/diff/timeline)
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

    /// Publish workflows (single upload / bulk upload)
    Publish {
        #[command(subcommand)]
        action: PublishAction,
    },

    /// Runtime operations (daemon/stream/hooks)
    Ops {
        #[command(subcommand)]
        action: OpsAction,
    },

    /// Account and server connectivity
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },

    /// Documentation helpers
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },

    /// Run E2E tests against a server (requires --features e2e)
    #[cfg(feature = "e2e")]
    Test(test_cmd::TestArgs),
}

#[derive(Subcommand)]
enum SessionAction {
    /// List all local AI sessions found on this machine
    Discover,
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
    /// Print timeline output as strings (pipe-friendly)
    Timeline {
        /// Session reference (HEAD, HEAD^N, ID) or direct session file path
        #[arg(default_value = "HEAD")]
        session: String,

        /// Restrict HEAD/ID lookup to a specific tool (e.g. claude, codex)
        #[arg(long)]
        tool: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "text")]
        format: tui_cmd::TuiOutputFormatArg,

        /// Timeline view mode
        #[arg(long, value_enum, default_value = "linear")]
        view: tui_cmd::TimelineViewArg,

        /// Disable collapsing of consecutive events
        #[arg(long)]
        no_collapse: bool,

        /// Actively generate timeline summaries before printing
        #[arg(long)]
        summaries: bool,

        /// Force summaries off for this render
        #[arg(long)]
        no_summary: bool,

        /// Override summary provider for this render
        #[arg(long)]
        summary_provider: Option<String>,

        /// Truncate rendered output to the first N rows
        #[arg(long)]
        max_rows: Option<usize>,
    },
}

#[derive(Subcommand)]
enum PublishAction {
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
}

#[derive(Subcommand)]
enum OpsAction {
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
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
    /// Manage git hooks integration
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
}

#[derive(Subcommand)]
enum AccountAction {
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
    /// Check server connection and authentication
    Server {
        #[command(subcommand)]
        action: ServerAction,
    },
}

#[derive(Subcommand)]
enum DocsAction {
    /// Generate shell completions
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

impl Commands {
    /// Whether this subcommand wants JSON-formatted error output.
    fn wants_json_errors(&self) -> bool {
        match self {
            Commands::Session { action } => match action {
                SessionAction::Log { format, json, .. } => {
                    matches!(format, OutputFormat::Json | OutputFormat::Stream) || json.is_some()
                }
                SessionAction::Handoff { format, .. } => {
                    matches!(
                        format,
                        OutputFormat::Json | OutputFormat::Stream | OutputFormat::Jsonl
                    )
                }
                SessionAction::Stats { format, .. } => matches!(format, OutputFormat::Json),
                SessionAction::Timeline { format, .. } => matches!(
                    format,
                    tui_cmd::TuiOutputFormatArg::Json | tui_cmd::TuiOutputFormatArg::Jsonl
                ),
                _ => false,
            },
            _ => false,
        }
    }
}

fn suggestion_for_code(code: &ExitCode) -> Option<&'static str> {
    match code {
        ExitCode::AuthError => Some("opensession account config --api-key <key>"),
        ExitCode::NoData => Some("opensession session index"),
        ExitCode::NetworkError => Some("opensession account server status"),
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
        Commands::Session {
            action: SessionAction::Discover,
        }
        | Commands::Publish {
            action: PublishAction::Upload { .. },
        }
        | Commands::Publish {
            action: PublishAction::UploadAll,
        } => {
            maybe_auto_start_daemon();
        }
        _ => {}
    }

    let json_errors = command.wants_json_errors();

    let result = match command {
        #[cfg(feature = "e2e")]
        Commands::Test(args) => test_cmd::run_test(args).await,
        Commands::Ui => opensession_tui::run(None),
        Commands::Session { action } => match action {
            SessionAction::Discover => discover::run_discover(),
            SessionAction::Index => index::run_index(),
            SessionAction::Log {
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
            SessionAction::Stats { period, format } => stats::run_stats(period, &format),
            SessionAction::Diff {
                session_a,
                session_b,
                ai,
            } => handoff::run_diff(&session_a, &session_b, ai).await,
            SessionAction::Handoff {
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
            SessionAction::Timeline {
                session,
                tool,
                format,
                view,
                no_collapse,
                summaries,
                no_summary,
                summary_provider,
                max_rows,
            } => tui_cmd::run_tui_timeline(
                &session,
                tool.as_deref(),
                format,
                view,
                no_collapse,
                summaries,
                no_summary,
                summary_provider.as_deref(),
                max_rows,
            ),
        },
        Commands::Publish { action } => match action {
            PublishAction::Upload { file, parent, git } => {
                upload::run_upload(&file, &parent, git).await
            }
            PublishAction::UploadAll => upload_all::run_upload_all().await,
        },
        Commands::Ops { action } => match action {
            OpsAction::Daemon { action } => match action {
                DaemonAction::Start => daemon_ctl::daemon_start(),
                DaemonAction::Stop => daemon_ctl::daemon_stop(),
                DaemonAction::Status => daemon_ctl::daemon_status(),
                DaemonAction::Health => run_daemon_health().await,
            },
            OpsAction::Stream { action } => run_stream_action(action),
            OpsAction::StreamPush { agent } => stream_push::run_stream_push(&agent),
            OpsAction::Hooks { action } => match action {
                HooksAction::Install => handoff::run_hooks_install(),
                HooksAction::Uninstall => handoff::run_hooks_uninstall(),
            },
        },
        Commands::Account { action } => match action {
            AccountAction::Config {
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
            AccountAction::Server { action } => match action {
                ServerAction::Status => server::run_status().await,
                ServerAction::Verify => server::run_verify().await,
            },
        },
        Commands::Docs { action } => match action {
            DocsAction::Completion { shell } => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
                Ok(())
            }
        },
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
