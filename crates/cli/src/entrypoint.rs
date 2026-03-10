use crate::{
    capture_cmd, cat_cmd, cleanup_cmd,
    cli_args::{Commands, parse_cli},
    config_cmd, docs_cmd, doctor_cmd, handoff_v1, inspect,
    locale::localize,
    parse_cmd, register, review, setup_cmd, share, summary_cmd, view,
};

pub(crate) async fn run_process() {
    let cli = parse_cli();

    let result = match cli.command {
        Commands::Register(args) => register::run(args),
        Commands::Capture(args) => capture_cmd::run(args),
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
        Commands::Docs { action } => docs_cmd::run_docs(action),
    };

    if let Err(error) = result {
        if debug_errors_enabled() {
            eprintln!("{} {error:#}", localize("Error:", "오류:"));
        } else {
            eprintln!("{} {error}", localize("Error:", "오류:"));
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
