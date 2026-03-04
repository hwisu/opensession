mod cat_cmd;
mod cleanup_cmd;
mod config_cmd;
mod doctor_cmd;
mod handoff_v1;
mod hooks;
mod inspect;
mod open_target;
mod parse_cmd;
mod register;
mod review;
mod runtime_settings;
mod setup_cmd;
mod share;
mod summary_cmd;
mod url_opener;
mod user_guidance;
mod view;

use clap::{Parser, Subcommand};
use std::path::Path;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "opensession",
    about = "OpenSession CLI - local-first source URI workflows",
    after_long_help = r"First-user flow (5 minutes):
  opensession docs quickstart

Common next steps:
  opensession doctor
  opensession doctor --fix
  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
  opensession register ./session.hail.jsonl
  opensession share os://src/local/<sha256> --git --remote origin"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register canonical HAIL JSONL into local object store.
    Register(register::RegisterArgs),
    /// Print canonical JSONL for a local source URI.
    Cat(cat_cmd::CatArgs),
    /// Inspect summary metadata for source/artifact URIs.
    Inspect(inspect::InspectArgs),
    /// Resolve sharing outputs from a source URI.
    Share(share::ShareArgs),
    /// Open a review-centric web view from URI/file/URL/commit targets.
    View(view::ViewArgs),
    /// Review a GitHub PR using local hidden refs and grouped commit sessions.
    Review(review::ReviewArgs),
    /// Build and manage immutable handoff artifacts.
    Handoff(handoff_v1::HandoffArgs),
    /// Parse agent-native logs into canonical HAIL JSONL.
    Parse(parse_cmd::ParseArgs),
    /// Generate/show local semantic summaries.
    Summary(summary_cmd::SummaryArgs),
    /// Manage explicit repo config (`.opensession/config.toml`).
    Config(config_cmd::ConfigArgs),
    /// Configure and run hidden-ref cleanup automation.
    Cleanup(cleanup_cmd::CleanupArgs),
    /// Install/update OpenSession git hooks and diagnostics.
    #[command(hide = true)]
    Setup(setup_cmd::SetupArgs),
    /// Diagnose and optionally fix local OpenSession setup.
    Doctor(doctor_cmd::DoctorArgs),
    /// Generate shell completion scripts.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
}

#[derive(Subcommand)]
enum DocsAction {
    /// Generate shell completions.
    Completion {
        /// Target shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print a 5-minute first-user flow.
    Quickstart {
        /// Parser profile used for first parse.
        #[arg(long, default_value = "codex")]
        profile: String,
        /// Raw input path for parse.
        #[arg(long, default_value = "./raw-session.jsonl")]
        input: PathBuf,
        /// Canonical output path for parse.
        #[arg(long, default_value = "./session.hail.jsonl")]
        out: PathBuf,
        /// Git remote name used for initial share.
        #[arg(long, default_value = "origin")]
        remote: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Register(args) => register::run(args),
        Commands::Cat(args) => cat_cmd::run(args),
        Commands::Inspect(args) => inspect::run(args),
        Commands::Share(args) => share::run(args),
        Commands::View(args) => view::run(args).await,
        Commands::Review(args) => review::run(args).await,
        Commands::Handoff(args) => handoff_v1::run(args),
        Commands::Parse(args) => parse_cmd::run(args),
        Commands::Summary(args) => summary_cmd::run(args).await,
        Commands::Config(args) => config_cmd::run(args),
        Commands::Cleanup(args) => cleanup_cmd::run(args),
        Commands::Setup(args) => setup_cmd::run(args),
        Commands::Doctor(args) => doctor_cmd::run(args),
        Commands::Docs { action } => run_docs(action),
    };

    if let Err(err) = result {
        if debug_errors_enabled() {
            eprintln!("Error: {err:#}");
        } else {
            eprintln!("Error: {err}");
        }
        std::process::exit(1);
    }
}

fn debug_errors_enabled() -> bool {
    matches!(
        std::env::var("OPENSESSION_DEBUG"),
        Ok(value)
            if matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
    )
}

fn run_docs(action: DocsAction) -> anyhow::Result<()> {
    match action {
        DocsAction::Completion { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
            Ok(())
        }
        DocsAction::Quickstart {
            profile,
            input,
            out,
            remote,
        } => {
            print_quickstart(&profile, &input, &out, &remote);
            Ok(())
        }
    }
}

fn print_quickstart(profile: &str, input: &Path, out: &Path, remote: &str) {
    println!("# OpenSession 5-minute first-user flow");
    println!();
    println!("# 1) Diagnose and apply setup");
    println!("opensession doctor");
    println!("opensession doctor --fix");
    println!();
    println!("# 2) Parse raw logs into canonical HAIL JSONL");
    println!(
        "opensession parse --profile {} {} --out {}",
        profile,
        input.display(),
        out.display()
    );
    println!();
    println!("# 3) Register canonical session locally");
    println!("opensession register {}", out.display());
    println!("# -> os://src/local/<sha256>");
    println!();
    println!("# 4) Share local source URI via git");
    println!(
        "opensession share os://src/local/<sha256> --git --remote {}",
        remote
    );
    println!();
    println!("# 5) Optional: convert a remote URI to web URL");
    println!("opensession config init --base-url https://opensession.io");
    println!("opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web");
}
