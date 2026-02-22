mod cat_cmd;
mod config_cmd;
mod handoff_v1;
mod inspect;
mod parse_cmd;
mod register;
mod share;
#[cfg(feature = "e2e")]
mod test_cmd;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "opensession",
    about = "OpenSession CLI - local-first source URI workflows"
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
    /// Build and manage immutable handoff artifacts.
    Handoff(handoff_v1::HandoffArgs),
    /// Parse agent-native logs into canonical HAIL JSONL.
    Parse(parse_cmd::ParseArgs),
    /// Manage explicit repo config (`.opensession/config.toml`).
    Config(config_cmd::ConfigArgs),
    /// Generate shell completion scripts.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
    /// Run E2E checks against a server (requires --features e2e).
    #[cfg(feature = "e2e")]
    Test(test_cmd::TestArgs),
}

#[derive(Subcommand)]
enum DocsAction {
    /// Generate shell completions.
    Completion {
        /// Target shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
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
        Commands::Handoff(args) => handoff_v1::run(args),
        Commands::Parse(args) => parse_cmd::run(args),
        Commands::Config(args) => config_cmd::run(args),
        Commands::Docs { action } => run_docs(action),
        #[cfg(feature = "e2e")]
        Commands::Test(args) => test_cmd::run_test(args).await,
    };

    if let Err(err) = result {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run_docs(action: DocsAction) -> anyhow::Result<()> {
    match action {
        DocsAction::Completion { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
            Ok(())
        }
    }
}
