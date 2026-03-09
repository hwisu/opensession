use std::path::Path;

use crate::{cli_args::command, locale::localize, setup_cmd};

pub(crate) fn run_docs(action: crate::cli_args::DocsAction) -> anyhow::Result<()> {
    match action {
        crate::cli_args::DocsAction::Completion { shell } => {
            let mut cmd = command();
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
    println!(
        "{}",
        localize(
            "# OpenSession 5-minute first-user flow",
            "# OpenSession 5분 첫 사용자 흐름",
        )
    );
    println!();
    println!(
        "{}",
        localize("# 1) Diagnose and apply setup", "# 1) 설정을 진단하고 적용")
    );
    println!("opensession doctor");
    println!(
        "opensession doctor --fix --profile {}",
        setup_profile.as_str()
    );
    if matches!(setup_profile, setup_cmd::SetupProfile::App) {
        println!("opensession doctor --fix --profile app --open-target app");
    }
    println!();
    println!(
        "{}",
        localize(
            "# 2) Parse raw logs into canonical HAIL JSONL",
            "# 2) raw 로그를 canonical HAIL JSONL로 변환",
        )
    );
    println!(
        "opensession parse --profile {} {} --out {}",
        profile,
        input.display(),
        out.display()
    );
    println!();
    println!(
        "{}",
        localize(
            "# 3) Register canonical session locally",
            "# 3) canonical 세션을 로컬에 등록",
        )
    );
    println!("opensession register {}", out.display());
    println!("# -> os://src/local/<sha256>");
    println!();
    println!(
        "{}",
        localize(
            "# 4) Share local source URI via quick git flow",
            "# 4) quick git 흐름으로 로컬 source URI 공유",
        )
    );
    println!(
        "opensession share os://src/local/<sha256> --quick --remote {}",
        remote
    );
    println!();
    println!(
        "{}",
        localize(
            "# 5) Optional: convert a remote URI to web URL",
            "# 5) 선택: remote URI를 웹 URL로 변환",
        )
    );
    println!("opensession config init --base-url https://opensession.io");
    println!("opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web");
}
