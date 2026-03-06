use clap::Parser;
use tracing::error;

use crate::cli::{Cli, DaemonCommand};

pub(crate) async fn run_process() {
    let cli = Cli::parse();
    initialize_tracing();

    let result = match cli.command.unwrap_or(DaemonCommand::Run) {
        DaemonCommand::Run => crate::runtime::run().await,
    };

    if let Err(error) = result {
        error!("Daemon fatal error: {:#}", error);
        std::process::exit(1);
    }
}

fn initialize_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("opensession_daemon=info".parse().unwrap())
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();
}
