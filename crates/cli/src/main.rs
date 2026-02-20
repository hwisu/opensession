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
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

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

    /// Background daemon controls (watch agents/repos + lifecycle)
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
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
    /// Generate handoff summary or manage handoff artifacts.
    Handoff {
        #[command(subcommand)]
        subcommand: Option<HandoffSubcommand>,

        #[command(flatten)]
        run: HandoffRunArgs,
    },
}

#[derive(Args, Debug, Clone, Default)]
struct HandoffInputArgs {
    /// Session file(s). Multiple files can be specified for merged handoff.
    files: Vec<PathBuf>,

    /// Use most recent session(s). Accepts optional count or HEAD~N (e.g. --last, --last 6, --last HEAD~6)
    #[arg(short, long, num_args = 0..=1, default_missing_value = "1", value_name = "N|HEAD~N")]
    last: Option<String>,

    /// Claude Code session reference (e.g. HEAD, HEAD~2)
    #[arg(long)]
    claude: Option<String>,

    /// Gemini session reference (e.g. HEAD, HEAD~1)
    #[arg(long)]
    gemini: Option<String>,

    /// Generic tool session reference (e.g. "amp HEAD~2")
    #[arg(long)]
    tool: Vec<String>,
}

#[derive(Args, Debug, Clone, Default)]
struct HandoffRunArgs {
    #[command(flatten)]
    input: HandoffInputArgs,

    /// Write output to a file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format.
    /// Defaults to markdown in terminal, json when piped.
    #[arg(long, value_enum)]
    format: Option<OutputFormat>,

    /// Validate handoff quality and print a validation report.
    #[arg(long)]
    validate: bool,

    /// Treat error-level validation findings as hard failures (non-zero exit).
    #[arg(long)]
    strict: bool,

    /// Populate HANDOFF.md via provider command: <provider[:model]> (e.g. claude, claude:opus-4.6)
    #[arg(long)]
    populate: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
enum HandoffSubcommand {
    /// Save merged handoff result as canonical ref artifact.
    Save {
        #[command(flatten)]
        input: HandoffInputArgs,

        /// Optional artifact id. When omitted, one is generated automatically.
        #[arg(long)]
        artifact_id: Option<String>,

        /// Payload format stored in artifact payload field.
        #[arg(long, value_enum, default_value_t = ArtifactPayloadFormatArg::Json)]
        payload_format: ArtifactPayloadFormatArg,
    },
    /// Manage saved handoff artifacts.
    Artifact {
        #[command(subcommand)]
        action: HandoffArtifactAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum HandoffArtifactAction {
    /// List handoff artifact refs.
    List,
    /// Show handoff artifact JSON by <artifact_id|ref>.
    Show {
        /// Artifact id or full ref (refs/opensession/handoff/artifacts/<id>)
        id_or_ref: String,
    },
    /// Refresh handoff artifact when sources are stale.
    Refresh {
        /// Artifact id or full ref (refs/opensession/handoff/artifacts/<id>)
        id_or_ref: String,
    },
    /// Render derived markdown from artifact.
    RenderMd {
        /// Artifact id or full ref (refs/opensession/handoff/artifacts/<id>)
        id_or_ref: String,
        /// Output markdown file path (default: HANDOFF.md).
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactPayloadFormatArg {
    Json,
    Jsonl,
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
enum AccountAction {
    /// Connect server/API key in one command
    Connect {
        /// Set the server URL
        #[arg(long)]
        server: Option<String>,

        /// Set the API key
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Show current account/server config
    Show,
    /// Check server health and version
    Status,
    /// Verify API key authentication
    Verify,
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
                SessionAction::Handoff { subcommand, run } => {
                    if subcommand.is_some() {
                        false
                    } else {
                        matches!(
                            run.format
                                .as_ref()
                                .unwrap_or(&default_handoff_format_for_output()),
                            OutputFormat::Json | OutputFormat::Stream | OutputFormat::Jsonl
                        )
                    }
                }
            },
            _ => false,
        }
    }
}

fn suggestion_for_code(code: &ExitCode) -> Option<&'static str> {
    match code {
        ExitCode::AuthError => Some("opensession account connect --server <url> [--api-key <key>]"),
        ExitCode::NoData => Some("opensession session handoff --last"),
        ExitCode::NetworkError => Some("opensession account status"),
        ExitCode::UsageError => Some("opensession --help"),
        _ => None,
    }
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the background daemon (optionally update watch targets first)
    Start {
        /// Repo directories to watch/upload
        #[arg(long)]
        repo: Vec<PathBuf>,
    },
    /// Stop the background daemon
    Stop,
    /// Show daemon status
    Status,
    /// Check daemon and server health
    Health,
    /// Update daemon watch targets (paths) without starting daemon
    Select {
        /// Repo directories to watch/upload
        #[arg(long)]
        repo: Vec<PathBuf>,
    },
    /// Show current daemon watch targets
    Show,
    /// Install stream-write hook manually for an agent
    EnableHook {
        /// Agent name (default: "claude-code")
        #[arg(long, default_value = "claude-code")]
        agent: String,
    },
    /// Stream new events from a local session file (hook target)
    StreamPush {
        /// Agent name (e.g. "claude-code")
        #[arg(long)]
        agent: String,
    },
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
        Commands::Daemon { action } => run_daemon_action(action).await,
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
        SessionAction::Handoff { subcommand, run } => match subcommand {
            Some(HandoffSubcommand::Save {
                input,
                artifact_id,
                payload_format,
            }) => {
                let input_last = input.last.clone();
                let force_last = should_default_to_last(
                    &input.files,
                    input_last.as_deref(),
                    input.claude.as_deref(),
                    input.gemini.as_deref(),
                    &input.tool,
                );
                let resolved_last = if input_last.is_none() && force_last {
                    Some("1".to_string())
                } else {
                    input_last
                };

                let payload_format = match payload_format {
                    ArtifactPayloadFormatArg::Json => {
                        opensession_core::handoff_artifact::HandoffPayloadFormat::Json
                    }
                    ArtifactPayloadFormatArg::Jsonl => {
                        opensession_core::handoff_artifact::HandoffPayloadFormat::Jsonl
                    }
                };

                handoff::run_handoff_save(
                    &input.files,
                    resolved_last.as_deref(),
                    input.claude.as_deref(),
                    input.gemini.as_deref(),
                    &input.tool,
                    artifact_id.as_deref(),
                    payload_format,
                )
                .await
            }
            Some(HandoffSubcommand::Artifact { action }) => match action {
                HandoffArtifactAction::List => handoff::run_handoff_artifact_list().await,
                HandoffArtifactAction::Show { id_or_ref } => {
                    handoff::run_handoff_artifact_show(&id_or_ref).await
                }
                HandoffArtifactAction::Refresh { id_or_ref } => {
                    handoff::run_handoff_artifact_refresh(&id_or_ref).await
                }
                HandoffArtifactAction::RenderMd { id_or_ref, output } => {
                    handoff::run_handoff_artifact_render_md(&id_or_ref, output.as_deref()).await
                }
            },
            None => {
                let run_last = run.input.last.clone();
                let format = run.format.unwrap_or_else(default_handoff_format_for_output);
                let force_last = should_default_to_last(
                    &run.input.files,
                    run_last.as_deref(),
                    run.input.claude.as_deref(),
                    run.input.gemini.as_deref(),
                    &run.input.tool,
                );
                let resolved_last = if run_last.is_none() && force_last {
                    Some("1".to_string())
                } else {
                    run_last
                };
                handoff::run_handoff(
                    &run.input.files,
                    resolved_last.as_deref(),
                    run.output.as_deref(),
                    format,
                    run.input.claude.as_deref(),
                    run.input.gemini.as_deref(),
                    &run.input.tool,
                    run.validate,
                    run.strict,
                    run.populate.as_deref(),
                )
                .await
            }
        },
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
    last: Option<&str>,
    claude: Option<&str>,
    gemini: Option<&str>,
    tools: &[String],
) -> bool {
    if last.is_some()
        || !files.is_empty()
        || claude.is_some()
        || gemini.is_some()
        || !tools.is_empty()
    {
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
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        match try_launch_external_tui(&scope) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(err) => {
                eprintln!(
                    "Warning: failed to launch opensession-tui ({err:#}). Falling back to text mode."
                );
            }
        }
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

    for candidate in opensession_tui_launch_candidates() {
        let status = match launch_tui_candidate(&candidate, &args) {
            Ok(status) => status,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to launch {}", candidate.display()));
            }
        };

        if status.success() {
            return Ok(true);
        }
        bail!("{} exited with status {status}", candidate.display());
    }

    Ok(false)
}

fn opensession_tui_launch_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let sibling = dir.join("opensession-tui");
            if sibling.exists() {
                candidates.push(sibling);
            }

            let sibling_exe = dir.join("opensession-tui.exe");
            if sibling_exe.exists() {
                candidates.push(sibling_exe);
            }
        }
    }

    candidates.push(PathBuf::from("opensession-tui"));
    candidates
}

fn launch_tui_candidate(
    candidate: &Path,
    args: &[String],
) -> std::io::Result<std::process::ExitStatus> {
    let mut retries = 0u8;
    loop {
        match Command::new(candidate).args(args).status() {
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock && retries < 1 => {
                retries += 1;
                std::thread::sleep(Duration::from_millis(120));
            }
            result => return result,
        }
    }
}

fn print_session_overview(repo_name: Option<String>) -> Result<()> {
    let db = opensession_local_db::LocalDb::open()?;
    let filter = opensession_local_db::LocalSessionFilter {
        git_repo_name: repo_name.clone(),
        limit: Some(120),
        ..Default::default()
    };
    let rows = db.list_sessions(&filter)?;
    let filtered_rows = rows.into_iter().take(30).collect::<Vec<_>>();

    if let Some(repo) = repo_name {
        println!("Scope: repo={repo}");
    } else {
        println!("Scope: local");
    }

    if filtered_rows.is_empty() {
        println!("No sessions found in this scope.");
        return Ok(());
    }

    println!(
        "{:<19}  {:<12}  {:>4}  {:<24}  Title",
        "Created", "Tool", "Msgs", "Repo"
    );
    for row in filtered_rows {
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

async fn run_daemon_action(action: DaemonAction) -> anyhow::Result<()> {
    match action {
        DaemonAction::Start { repo } => {
            if !repo.is_empty() {
                update_daemon_targets(&repo)?;
            }
            daemon_ctl::daemon_start()
        }
        DaemonAction::Stop => daemon_ctl::daemon_stop(),
        DaemonAction::Status => daemon_ctl::daemon_status(),
        DaemonAction::Health => run_daemon_health().await,
        DaemonAction::Select { repo } => {
            if repo.is_empty() {
                bail!("No changes requested. Use --repo.");
            }
            update_daemon_targets(&repo)?;
            print_daemon_targets()
        }
        DaemonAction::Show => print_daemon_targets(),
        DaemonAction::EnableHook { agent } => stream_push::enable_stream_write(&agent),
        DaemonAction::StreamPush { agent } => stream_push::run_stream_push(&agent),
    }
}

async fn run_account_action(action: AccountAction) -> anyhow::Result<()> {
    match action {
        AccountAction::Connect { server, api_key } => {
            if server.is_none() && api_key.is_none() {
                config::show_config()
            } else {
                config::set_config(server, api_key)
            }
        }
        AccountAction::Show => config::show_config(),
        AccountAction::Status => server::run_status().await,
        AccountAction::Verify => server::run_verify().await,
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

fn update_daemon_targets(repo_flags: &[PathBuf]) -> anyhow::Result<()> {
    let current_repos = config::daemon_watch_paths()?;

    let next_repos = if repo_flags.is_empty() {
        current_repos
    } else {
        normalize_repo_flags(repo_flags)?
    };

    config::set_daemon_watch_paths(next_repos)
}

fn normalize_repo_flags(repo_flags: &[PathBuf]) -> anyhow::Result<Vec<String>> {
    let mut repos = Vec::new();
    let mut seen = HashSet::new();

    for raw in repo_flags {
        let canonical = std::fs::canonicalize(raw)
            .with_context(|| format!("Repo path not found: {}", raw.display()))?;
        if !canonical.is_dir() {
            bail!("Repo path must be a directory: {}", canonical.display());
        }
        let path = canonical.to_string_lossy().to_string();
        if seen.insert(path.clone()) {
            repos.push(path);
        }
    }

    Ok(repos)
}

fn print_daemon_targets() -> anyhow::Result<()> {
    let path = config::config_path()?;
    let repos = config::daemon_watch_paths()?;
    println!("Config file: {}", path.display());
    println!();
    println!("[daemon.watchers]");
    if repos.is_empty() {
        println!("  repos       = (none)");
    } else {
        println!("  repos:");
        for repo in repos {
            println!("    - {repo}");
        }
    }
    println!();
    println!("Tip: use TUI for manual control: `opensession` or `opensession .`");
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

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, DaemonAction};
    use clap::Parser;
    use std::{fs, path::Path};

    #[test]
    fn daemon_enable_hook_subcommand_parses() {
        let cli = Cli::parse_from([
            "opensession",
            "daemon",
            "enable-hook",
            "--agent",
            "claude-code",
        ]);
        match cli.command {
            Some(Commands::Daemon {
                action: DaemonAction::EnableHook { agent },
            }) => assert_eq!(agent, "claude-code"),
            _ => panic!("unexpected command parse result"),
        }
    }

    #[test]
    fn workspace_members_include_tui_crate() {
        let workspace_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let manifest_str = fs::read_to_string(&workspace_manifest)
            .expect("read workspace Cargo.toml for membership check");
        let manifest: toml::Value =
            toml::from_str(&manifest_str).expect("parse workspace Cargo.toml");
        let members = manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(toml::Value::as_array)
            .expect("workspace.members must exist");

        assert!(
            members
                .iter()
                .any(|value| value.as_str() == Some("crates/tui")),
            "workspace.members must include crates/tui"
        );
    }
}
