use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "opensession-daemon",
    about = "Background daemon for automatic session detection and local indexing"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<DaemonCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DaemonCommand {
    /// Start the daemon event loop.
    Run,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_run_when_no_subcommand() {
        let cli = Cli::try_parse_from(["opensession-daemon"]).expect("parse cli");
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_accepts_run_subcommand() {
        let cli = Cli::try_parse_from(["opensession-daemon", "run"]).expect("parse cli");
        assert!(matches!(cli.command, Some(DaemonCommand::Run)));
    }
}
