mod config;
mod daemon_ctl;
mod handoff;
mod output;
pub mod server;
mod session_ref;
mod stream_push;
#[cfg(feature = "e2e")]
mod test_cmd;
mod upload;
mod upload_all;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::output::OutputFormat;

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
    about = "opensession.io CLI - handoff and sharing workflows",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Scope path for interactive mode (`.` = current repo, omitted = all local sessions)
    scope: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Session workflows
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

    /// Publish workflows (single upload / bulk upload)
    Publish {
        #[command(subcommand)]
        action: PublishAction,
    },

    /// Runtime operations (daemon/stream)
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
        /// Defaults to markdown in terminal, json when piped.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,

        /// Claude Code session reference (e.g. HEAD, HEAD~2)
        #[arg(long)]
        claude: Option<String>,

        /// Gemini session reference (e.g. HEAD, HEAD~1)
        #[arg(long)]
        gemini: Option<String>,

        /// Generic tool session reference (e.g. "amp HEAD~2")
        #[arg(long)]
        tool: Vec<String>,
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
                SessionAction::Handoff { format, .. } => {
                    matches!(
                        format
                            .as_ref()
                            .unwrap_or(&default_handoff_format_for_output()),
                        OutputFormat::Json | OutputFormat::Stream | OutputFormat::Jsonl
                    )
                }
            },
            _ => false,
        }
    }
}

fn suggestion_for_code(code: &ExitCode) -> Option<&'static str> {
    match code {
        ExitCode::AuthError => Some("opensession account config --api-key <key>"),
        ExitCode::NoData => Some("opensession session handoff --last"),
        ExitCode::NetworkError => Some("opensession account server status"),
        ExitCode::UsageError => Some("opensession --help"),
        _ => None,
    }
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

enum InteractiveScope {
    AllLocal,
    Repo {
        repo_name: String,
        paths: Vec<String>,
    },
    File {
        path: String,
    },
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

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            let scope = match resolve_interactive_scope(cli.scope.as_deref()) {
                Ok(scope) => scope,
                Err(e) => {
                    eprintln!("Error: {e:#}");
                    std::process::exit(ExitCode::UsageError as i32);
                }
            };

            if let Err(e) = run_interactive_entry(scope) {
                eprintln!("Error: {e:#}");
                std::process::exit(ExitCode::GeneralError as i32);
            }
            return;
        }
    };

    // Auto-start daemon for commands that benefit from it
    match &command {
        Commands::Publish {
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
        Commands::Session { action } => run_session_action(action).await,
        Commands::Publish { action } => run_publish_action(action).await,
        Commands::Ops { action } => run_ops_action(action).await,
        Commands::Account { action } => run_account_action(action).await,
        Commands::Docs { action } => run_docs_action(action),
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

async fn run_session_action(action: SessionAction) -> anyhow::Result<()> {
    match action {
        SessionAction::Handoff {
            files,
            last,
            output,
            format,
            claude,
            gemini,
            tool,
        } => {
            let format = format.unwrap_or_else(default_handoff_format_for_output);
            let force_last =
                should_default_to_last(&files, last, claude.as_deref(), gemini.as_deref(), &tool);
            handoff::run_handoff(
                &files,
                last || force_last,
                output.as_deref(),
                format,
                claude.as_deref(),
                gemini.as_deref(),
                &tool,
            )
            .await
        }
    }
}

fn default_handoff_format_for_output() -> OutputFormat {
    if std::io::stdout().is_terminal() {
        OutputFormat::Markdown
    } else {
        OutputFormat::Json
    }
}

fn should_default_to_last(
    files: &[PathBuf],
    last: bool,
    claude: Option<&str>,
    gemini: Option<&str>,
    tools: &[String],
) -> bool {
    if last || !files.is_empty() || claude.is_some() || gemini.is_some() || !tools.is_empty() {
        return false;
    }

    !std::io::stdout().is_terminal()
}

fn resolve_interactive_scope(scope: Option<&Path>) -> Result<InteractiveScope> {
    let Some(scope) = scope else {
        return Ok(InteractiveScope::AllLocal);
    };

    let canonical = std::fs::canonicalize(scope)
        .with_context(|| format!("Path not found: {}", scope.display()))?;

    if canonical.is_file() {
        return Ok(InteractiveScope::File {
            path: canonical.to_string_lossy().into_owned(),
        });
    }

    if !canonical.is_dir() {
        bail!(
            "Scope must be a file or directory path, got: {}",
            canonical.display()
        );
    }

    let git_ctx =
        opensession_local_db::git::extract_git_context(canonical.to_string_lossy().as_ref());
    let repo_name = git_ctx.repo_name.with_context(|| {
        format!(
            "{} is not inside a git repository. Use `opensession` for all local sessions.",
            canonical.display()
        )
    })?;

    let db = opensession_local_db::LocalDb::open()?;
    let filter = opensession_local_db::LocalSessionFilter {
        git_repo_name: Some(repo_name.clone()),
        ..Default::default()
    };
    let rows = db.list_sessions(&filter)?;

    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for row in rows {
        let Some(source_path) = row.source_path else {
            continue;
        };
        if source_path.trim().is_empty() || !seen.insert(source_path.clone()) {
            continue;
        }
        if Path::new(&source_path).exists() {
            paths.push(source_path);
        }
    }

    Ok(InteractiveScope::Repo { repo_name, paths })
}

fn run_interactive_entry(scope: InteractiveScope) -> Result<()> {
    if try_launch_external_tui(&scope)? {
        return Ok(());
    }

    match scope {
        InteractiveScope::File { .. } => {
            bail!("`opensession-tui` is not installed. For file input, use `opensession session handoff <file>`.");
        }
        InteractiveScope::AllLocal => print_session_overview(None)?,
        InteractiveScope::Repo { repo_name, .. } => print_session_overview(Some(repo_name))?,
    }

    Ok(())
}

fn try_launch_external_tui(scope: &InteractiveScope) -> Result<bool> {
    let args: Vec<String> = match scope {
        InteractiveScope::AllLocal => Vec::new(),
        InteractiveScope::Repo { paths, .. } => {
            if paths.is_empty() {
                return Ok(false);
            }
            paths.clone()
        }
        InteractiveScope::File { path } => vec![path.clone()],
    };

    let status = match Command::new("opensession-tui").args(&args).status() {
        Ok(status) => status,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).context("failed to launch opensession-tui"),
    };

    if status.success() {
        Ok(true)
    } else {
        bail!("opensession-tui exited with status {status}");
    }
}

fn print_session_overview(repo_name: Option<String>) -> Result<()> {
    let db = opensession_local_db::LocalDb::open()?;
    let filter = opensession_local_db::LocalSessionFilter {
        git_repo_name: repo_name.clone(),
        limit: Some(30),
        ..Default::default()
    };
    let rows = db.list_sessions(&filter)?;

    if let Some(repo) = repo_name {
        println!("Scope: repo={repo}");
    } else {
        println!("Scope: local");
    }

    if rows.is_empty() {
        println!("No sessions found in this scope.");
        return Ok(());
    }

    println!(
        "{:<19}  {:<12}  {:>4}  {:<24}  Title",
        "Created", "Tool", "Msgs", "Repo"
    );
    for row in rows {
        let created = row
            .created_at
            .split('T')
            .next()
            .unwrap_or(row.created_at.as_str())
            .to_string();
        let repo = row.git_repo_name.unwrap_or_else(|| "-".to_string());
        let title = row.title.unwrap_or_else(|| row.id.clone());
        println!(
            "{:<19}  {:<12}  {:>4}  {:<24}  {}",
            created,
            row.tool,
            row.message_count,
            truncate_display(&repo, 24),
            truncate_display(&title, 80)
        );
    }
    println!();
    println!("Tip: install `opensession-tui` to launch interactive mode from `opensession`.");
    Ok(())
}

fn truncate_display(value: &str, max: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max {
        return value.to_string();
    }
    chars[..max.saturating_sub(3)].iter().collect::<String>() + "..."
}

async fn run_publish_action(action: PublishAction) -> anyhow::Result<()> {
    match action {
        PublishAction::Upload { file, parent, git } => {
            upload::run_upload(&file, &parent, git).await
        }
        PublishAction::UploadAll => upload_all::run_upload_all().await,
    }
}

async fn run_ops_action(action: OpsAction) -> anyhow::Result<()> {
    match action {
        OpsAction::Daemon { action } => match action {
            DaemonAction::Start => daemon_ctl::daemon_start(),
            DaemonAction::Stop => daemon_ctl::daemon_stop(),
            DaemonAction::Status => daemon_ctl::daemon_status(),
            DaemonAction::Health => run_daemon_health().await,
        },
        OpsAction::Stream { action } => run_stream_action(action),
        OpsAction::StreamPush { agent } => stream_push::run_stream_push(&agent),
    }
}

async fn run_account_action(action: AccountAction) -> anyhow::Result<()> {
    match action {
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
    }
}

fn run_docs_action(action: DocsAction) -> anyhow::Result<()> {
    match action {
        DocsAction::Completion { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
            Ok(())
        }
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
