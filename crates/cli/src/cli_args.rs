use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::setup_cmd;

#[derive(Parser)]
#[command(
    name = "opensession",
    about = "OpenSession CLI - local-first source URI workflows",
    after_long_help = r"First-user flow (5 minutes):
  opensession docs quickstart

Common next steps:
  opensession doctor
  opensession doctor --fix --profile local
  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
  opensession register ./session.hail.jsonl
  opensession share os://src/local/<sha256> --quick"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Register canonical HAIL JSONL into local object store.
    Register(crate::register::RegisterArgs),
    /// Print canonical JSONL for a local source URI.
    Cat(crate::cat_cmd::CatArgs),
    /// Inspect summary metadata for source/artifact URIs.
    Inspect(crate::inspect::InspectArgs),
    /// Resolve sharing outputs from a source URI.
    Share(crate::share::ShareArgs),
    /// Open a review-centric web view from URI/file/URL/commit targets.
    View(crate::view::ViewArgs),
    /// Review a GitHub PR using local hidden refs and grouped commit sessions.
    Review(crate::review::ReviewArgs),
    /// Build and manage immutable handoff artifacts.
    Handoff(crate::handoff_v1::HandoffArgs),
    /// Parse agent-native logs into canonical HAIL JSONL.
    Parse(crate::parse_cmd::ParseArgs),
    /// Generate/show local semantic summaries.
    Summary(crate::summary_cmd::SummaryArgs),
    /// Manage explicit repo config (`.opensession/config.toml`).
    Config(crate::config_cmd::ConfigArgs),
    /// Configure and run hidden-ref cleanup automation.
    Cleanup(crate::cleanup_cmd::CleanupArgs),
    /// Install/update OpenSession git hooks and diagnostics.
    #[command(hide = true)]
    Setup(crate::setup_cmd::SetupArgs),
    /// Diagnose and optionally fix local OpenSession setup.
    Doctor(crate::doctor_cmd::DoctorArgs),
    /// Generate shell completion scripts.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
}

#[derive(Subcommand)]
pub(crate) enum DocsAction {
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
        /// Setup profile used for doctor/setup defaults.
        #[arg(long, value_enum, default_value = "local")]
        setup_profile: setup_cmd::SetupProfile,
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

#[cfg(test)]
mod tests {
    use clap::Parser;
    use std::path::PathBuf;

    use super::{Cli, Commands, DocsAction};

    #[test]
    fn parses_docs_completion_subcommand() {
        let cli = Cli::parse_from(["opensession", "docs", "completion", "zsh"]);
        match cli.command {
            Commands::Docs {
                action: DocsAction::Completion { shell },
            } => {
                assert_eq!(shell.to_string(), "zsh");
            }
            _ => panic!("expected docs completion command"),
        }
    }

    #[test]
    fn quickstart_defaults_profile_and_remote() {
        let cli = Cli::parse_from(["opensession", "docs", "quickstart"]);
        match cli.command {
            Commands::Docs {
                action:
                    DocsAction::Quickstart {
                        profile,
                        remote,
                        input,
                        out,
                        ..
                    },
            } => {
                assert_eq!(profile, "codex");
                assert_eq!(remote, "origin");
                assert_eq!(input, PathBuf::from("./raw-session.jsonl"));
                assert_eq!(out, PathBuf::from("./session.hail.jsonl"));
            }
            _ => panic!("expected docs quickstart command"),
        }
    }
}
