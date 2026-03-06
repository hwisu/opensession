use clap::CommandFactory;
use std::path::Path;

use crate::{cli_args::Cli, setup_cmd};

pub(crate) fn run_docs(action: crate::cli_args::DocsAction) -> anyhow::Result<()> {
    match action {
        crate::cli_args::DocsAction::Completion { shell } => {
            let mut cmd = <Cli as CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "opensession", &mut std::io::stdout());
            Ok(())
        }
        crate::cli_args::DocsAction::Quickstart {
            profile,
            setup_profile,
            input,
            out,
            remote,
        } => {
            print_quickstart(&profile, setup_profile, &input, &out, &remote);
            Ok(())
        }
    }
}

fn print_quickstart(
    profile: &str,
    setup_profile: setup_cmd::SetupProfile,
    input: &Path,
    out: &Path,
    remote: &str,
) {
    println!("# OpenSession 5-minute first-user flow");
    println!();
    println!("# 1) Diagnose and apply setup");
    println!("opensession doctor");
    println!(
        "opensession doctor --fix --profile {}",
        setup_profile.as_str()
    );
    if matches!(setup_profile, setup_cmd::SetupProfile::App) {
        println!("opensession doctor --fix --profile app --open-target app");
    }
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
    println!("# 4) Share local source URI via quick git flow");
    println!(
        "opensession share os://src/local/<sha256> --quick --remote {}",
        remote
    );
    println!();
    println!("# 5) Optional: convert a remote URI to web URL");
    println!("opensession config init --base-url https://opensession.io");
    println!("opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web");
}
