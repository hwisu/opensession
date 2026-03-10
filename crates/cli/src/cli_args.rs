use clap::{Command, CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;

use crate::{locale::localize, setup_cmd};

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
    /// Import native logs plus job metadata into the local review ledger.
    Capture(crate::capture_cmd::CaptureArgs),
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

pub(crate) fn command() -> Command {
    let mut command = <Cli as CommandFactory>::command();
    localize_command(&mut command);
    command
}

pub(crate) fn parse_cli() -> Cli {
    let matches = command().get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
}

fn set_about(command: &mut Command, text: &'static str) {
    *command = command.clone().about(text);
}

fn set_after_help(command: &mut Command, text: &'static str) {
    *command = command.clone().after_help(text);
}

fn localize_command(command: &mut Command) {
    match command.get_name() {
        "opensession" => {
            set_about(
                command,
                localize(
                    "OpenSession CLI - local-first source URI workflows",
                    "OpenSession CLI - 로컬 우선 Source URI 워크플로",
                ),
            );
            set_after_help(
                command,
                localize(
                    "First-user flow (5 minutes):\n  opensession docs quickstart\n\nCommon next steps:\n  opensession doctor\n  opensession doctor --fix --profile local\n  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl\n  opensession register ./session.hail.jsonl\n  opensession share os://src/local/<sha256> --quick",
                    "첫 사용자 흐름 (5분):\n  opensession docs quickstart\n\n다음으로 많이 쓰는 명령:\n  opensession doctor\n  opensession doctor --fix --profile local\n  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl\n  opensession register ./session.hail.jsonl\n  opensession share os://src/local/<sha256> --quick",
                ),
            );
        }
        "register" => {
            set_about(
                command,
                localize(
                    "Register canonical HAIL JSONL into local object store.",
                    "canonical HAIL JSONL을 로컬 객체 저장소에 등록합니다.",
                ),
            );
        }
        "capture" => {
            set_about(
                command,
                localize(
                    "Import native logs plus job metadata into the local review ledger.",
                    "native 로그와 job 메타데이터를 로컬 review ledger로 가져옵니다.",
                ),
            );
        }
        "cat" => {
            set_about(
                command,
                localize(
                    "Print canonical JSONL for a local source URI.",
                    "로컬 source URI의 canonical JSONL을 출력합니다.",
                ),
            );
        }
        "inspect" => {
            set_about(
                command,
                localize(
                    "Inspect summary metadata for source/artifact URIs.",
                    "source/artifact URI의 summary 메타데이터를 확인합니다.",
                ),
            );
        }
        "share" => {
            set_about(
                command,
                localize(
                    "Resolve sharing outputs from a source URI.",
                    "Source URI에서 공유 출력을 생성합니다.",
                ),
            );
        }
        "view" => {
            set_about(
                command,
                localize(
                    "Open a review-centric web view from URI/file/URL/commit targets.",
                    "URI/파일/URL/커밋 대상을 리뷰 중심 웹 보기로 엽니다.",
                ),
            );
            set_after_help(
                command,
                localize(
                    "Recovery examples:\n  opensession view --no-open\n  opensession view os://src/local/<sha256> --no-open\n  opensession view ./session.hail.jsonl --no-open\n  opensession view HEAD~3..HEAD --no-open",
                    "복구 예시:\n  opensession view --no-open\n  opensession view os://src/local/<sha256> --no-open\n  opensession view ./session.hail.jsonl --no-open\n  opensession view HEAD~3..HEAD --no-open",
                ),
            );
        }
        "review" => {
            set_about(
                command,
                localize(
                    "Review a GitHub PR using local hidden refs and grouped commit sessions.",
                    "로컬 hidden ref와 grouped commit session을 사용해 GitHub PR을 검토합니다.",
                ),
            );
        }
        "handoff" => {
            set_about(
                command,
                localize(
                    "Build and manage immutable handoff artifacts.",
                    "불변 handoff artifact를 생성하고 관리합니다.",
                ),
            );
        }
        "parse" => {
            set_about(
                command,
                localize(
                    "Parse agent-native logs into canonical HAIL JSONL.",
                    "agent-native 로그를 canonical HAIL JSONL로 변환합니다.",
                ),
            );
            set_after_help(
                command,
                localize(
                    "Recovery examples:\n  opensession parse --profile codex ./raw-session.jsonl --preview\n  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl",
                    "복구 예시:\n  opensession parse --profile codex ./raw-session.jsonl --preview\n  opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl",
                ),
            );
        }
        "summary" => {
            set_about(
                command,
                localize(
                    "Generate/show local semantic summaries.",
                    "로컬 시맨틱 summary를 생성하거나 표시합니다.",
                ),
            );
        }
        "config" => {
            set_about(
                command,
                localize(
                    "Manage explicit repo config (`.opensession/config.toml`).",
                    "명시적 레포 설정(`.opensession/config.toml`)을 관리합니다.",
                ),
            );
        }
        "cleanup" => {
            set_about(
                command,
                localize(
                    "Configure and run hidden-ref cleanup automation.",
                    "hidden-ref cleanup 자동화를 구성하고 실행합니다.",
                ),
            );
        }
        "setup" => {
            set_about(
                command,
                localize(
                    "Install/update OpenSession git hooks and diagnostics.",
                    "OpenSession git hook과 진단 구성을 설치/업데이트합니다.",
                ),
            );
        }
        "doctor" => {
            set_about(
                command,
                localize(
                    "Diagnose and optionally fix local OpenSession setup.",
                    "로컬 OpenSession 설정을 진단하고 필요하면 수정합니다.",
                ),
            );
        }
        "docs" => {
            set_about(
                command,
                localize(
                    "Print shell completions and quickstart guidance.",
                    "셸 completion과 빠른 시작 안내를 출력합니다.",
                ),
            );
        }
        "completion" => {
            set_about(
                command,
                localize("Generate shell completions.", "셸 completion을 생성합니다."),
            );
        }
        "quickstart" => {
            set_about(
                command,
                localize(
                    "Print a 5-minute first-user flow.",
                    "5분짜리 첫 사용자 흐름을 출력합니다.",
                ),
            );
        }
        _ => {}
    }

    for subcommand in command.get_subcommands_mut() {
        localize_command(subcommand);
    }
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
