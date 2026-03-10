use super::*;

#[test]
fn help_shows_v1_commands() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(stdout.contains("register"));
    assert!(stdout.contains("share"));
    assert!(stdout.contains("view"));
    assert!(stdout.contains("handoff"));
    assert!(!stdout.contains("\n  setup  "));
    assert!(!stdout.contains("publish"));
    assert!(stdout.contains("first-user flow (5 minutes):"));
    assert!(stdout.contains("opensession docs quickstart"));
    assert!(stdout.contains("common next steps:"));
    assert!(stdout.contains("opensession doctor --fix"));
}

#[test]
fn parse_help_shows_recovery_examples() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["parse", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Recovery examples:"));
    assert!(stdout.contains("opensession parse --profile codex ./raw-session.jsonl --preview"));
    assert!(stdout.contains(
        "opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl"
    ));
}

#[test]
fn share_help_shows_recovery_examples() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["share", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Recovery examples:"));
    assert!(stdout.contains("opensession share os://src/local/<sha256> --git --remote origin"));
    assert!(stdout.contains(
        "opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web"
    ));
}

#[test]
fn view_help_shows_recovery_examples() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["view", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Recovery examples:"));
    assert!(stdout.contains("opensession view --no-open"));
    assert!(stdout.contains("opensession view os://src/local/<sha256> --no-open"));
    assert!(stdout.contains("opensession view ./session.hail.jsonl --no-open"));
    assert!(stdout.contains("opensession view HEAD~3..HEAD --no-open"));
}

#[test]
fn doctor_help_shows_recovery_examples() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["doctor", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Recovery examples:"));
    assert!(stdout.contains("opensession doctor --fix --profile local"));
    assert!(stdout.contains("opensession doctor --fix --yes --profile app"));
    assert!(stdout.contains("opensession docs quickstart"));
}

#[test]
fn doctor_yes_without_fix_shows_next_steps() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["doctor", "--yes"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("`--yes` requires `--fix`"));
    assert!(stderr.contains("next:"));
    assert!(
        stderr.contains("opensession doctor --fix --yes --profile local --fanout-mode hidden_ref")
    );
}

#[test]
fn doctor_fanout_without_fix_shows_next_steps() {
    let tmp = make_home();
    let output = run(
        tmp.path(),
        tmp.path(),
        &["doctor", "--fanout-mode", "hidden_ref"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("`--fanout-mode` requires `--fix`"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession doctor --fix --fanout-mode hidden_ref"));
}

#[test]
fn docs_completion_still_available() {
    let tmp = make_home();
    let out = run(tmp.path(), tmp.path(), &["docs", "completion", "bash"]);
    assert!(out.status.success());
    assert!(!out.stdout.is_empty());
}

#[test]
fn docs_quickstart_prints_first_user_flow() {
    let tmp = make_home();
    let out = run(tmp.path(), tmp.path(), &["docs", "quickstart"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("OpenSession 5-minute first-user flow"));
    assert!(stdout.contains("opensession doctor --fix"));
    assert!(stdout.contains("opensession parse --profile codex"));
    assert!(stdout.contains("opensession register ./session.hail.jsonl"));
    assert!(stdout.contains("opensession share os://src/local/<sha256> --quick --remote origin"));
}

#[test]
fn testing_helper_agent_is_available() {
    // Keep one assertion using opensession_core::testing to ensure dev-dependency path stays valid.
    let agent = testing::agent();
    assert!(!agent.tool.is_empty());
}
