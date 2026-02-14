use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::process::Command as StdCommand;

fn make_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, body).expect("write file");
}

fn create_codex_session(home: &Path, rel: &str) -> PathBuf {
    let path = home.join(".codex").join("sessions").join(rel);
    write_file(&path, "{\"type\":\"noop\"}\n");
    path
}

fn create_claude_session(home: &Path, rel: &str) -> PathBuf {
    let path = home.join(".claude").join("projects").join(rel);
    write_file(&path, "{\"type\":\"noop\"}\n");
    path
}

#[cfg(unix)]
fn set_file_mtime(path: &Path, timestamp: &str) {
    let status = StdCommand::new("touch")
        .arg("-t")
        .arg(timestamp)
        .arg(path)
        .status()
        .expect("set file mtime");
    assert!(status.success(), "touch -t should succeed");
}

fn parse_dry_run(output: &[u8]) -> Value {
    serde_json::from_slice(output).expect("dry-run output json")
}

fn run_view(home: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.args(args)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env_remove("OPS_TL_SUM_CLI_BIN")
        .env_remove("OPS_TL_SUM_CLI_ARGS")
        .env_remove("OPS_TL_SUM_MODEL")
        .env_remove("OPS_TL_SUM_ENDPOINT")
        .env_remove("OPS_TL_SUM_BASE")
        .env_remove("OPS_TL_SUM_PATH")
        .env_remove("OPS_TL_SUM_STYLE")
        .env_remove("OPS_TL_SUM_KEY")
        .env_remove("OPS_TL_SUM_KEY_HEADER");
    cmd.output().expect("run opensession view")
}

#[test]
fn view_help_lists_runtime_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_opensession"))
        .args(["view", "--help"])
        .output()
        .expect("run help");
    assert!(output.status.success(), "help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--active-within-minutes"));
    assert!(stdout.contains("--summary-provider"));
    assert!(stdout.contains("--sum-endpoint"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn view_dry_run_maps_claude_alias_to_claude_code() {
    let tmp = make_home();
    let home = tmp.path();
    create_claude_session(home, "proj-a/session-a.jsonl");

    let output = run_view(
        home,
        &[
            "view",
            "claude",
            "--dry-run",
            "--non-interactive",
            "--summary-provider",
            "auto",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_dry_run(&output.stdout);
    assert_eq!(
        json.get("agent").and_then(|v| v.as_str()),
        Some("claude-code")
    );
}

#[test]
fn view_selects_latest_when_multiple_active_candidates_exist() {
    let tmp = make_home();
    let home = tmp.path();
    let older = create_codex_session(home, "2026/02/14/old.jsonl");
    thread::sleep(Duration::from_millis(1200));
    let newer = create_codex_session(home, "2026/02/14/new.jsonl");

    let output = run_view(
        home,
        &[
            "view",
            "codex",
            "--dry-run",
            "--non-interactive",
            "--active-within-minutes",
            "120",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_dry_run(&output.stdout);
    let selected = json
        .get("selected_path")
        .and_then(|v| v.as_str())
        .expect("selected path");
    assert_eq!(selected, newer.to_string_lossy());
    assert_ne!(selected, older.to_string_lossy());
    assert_eq!(
        json.get("selection_mode").and_then(|v| v.as_str()),
        Some("active-window")
    );
}

#[test]
fn view_falls_back_to_latest_when_no_active_candidates() {
    let tmp = make_home();
    let home = tmp.path();
    let older = create_codex_session(home, "2026/02/14/fallback-old.jsonl");
    thread::sleep(Duration::from_millis(1200));
    let newer = create_codex_session(home, "2026/02/14/fallback-new.jsonl");

    let output = run_view(
        home,
        &[
            "view",
            "codex",
            "--dry-run",
            "--non-interactive",
            "--active-within-minutes",
            "0",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_dry_run(&output.stdout);
    let selected = json
        .get("selected_path")
        .and_then(|v| v.as_str())
        .expect("selected path");
    assert_eq!(selected, newer.to_string_lossy());
    assert_ne!(selected, older.to_string_lossy());
    assert_eq!(
        json.get("selection_mode").and_then(|v| v.as_str()),
        Some("fallback-latest")
    );
}

#[cfg(unix)]
#[test]
fn view_sort_is_deterministic_for_tied_mtime_files() {
    let tmp = make_home();
    let home = tmp.path();

    let later_by_name = create_codex_session(home, "2026/02/14/z-file.jsonl");
    let earlier_by_name = create_codex_session(home, "2026/02/14/a-file.jsonl");

    set_file_mtime(&later_by_name, "203012312359.00");
    set_file_mtime(&earlier_by_name, "203012312359.00");

    let output = run_view(
        home,
        &[
            "view",
            "codex",
            "--dry-run",
            "--non-interactive",
            "--active-within-minutes",
            "120",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_dry_run(&output.stdout);
    let selected = json
        .get("selected_path")
        .and_then(|v| v.as_str())
        .expect("selected path");
    assert_eq!(selected, earlier_by_name.to_string_lossy());
}

#[test]
fn view_dry_run_reflects_summary_overrides() {
    let tmp = make_home();
    let home = tmp.path();
    create_codex_session(home, "2026/02/14/override.jsonl");

    let output = run_view(
        home,
        &[
            "view",
            "codex",
            "--dry-run",
            "--non-interactive",
            "--summary-provider",
            "openai-compatible",
            "--summary-model",
            "gpt-4o-mini",
            "--sum-endpoint",
            "https://example.com/v1/responses",
            "--sum-base",
            "https://example.com/v1",
            "--sum-path",
            "/responses",
            "--sum-style",
            "responses",
            "--sum-key",
            "sk-test",
            "--sum-key-header",
            "Authorization",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_dry_run(&output.stdout);
    assert_eq!(
        json.get("summary_provider").and_then(|v| v.as_str()),
        Some("openai-compatible")
    );
    assert_eq!(
        json.get("summary_model").and_then(|v| v.as_str()),
        Some("gpt-4o-mini")
    );
    assert_eq!(
        json.get("sum_endpoint").and_then(|v| v.as_str()),
        Some("https://example.com/v1/responses")
    );
    assert_eq!(
        json.get("sum_style").and_then(|v| v.as_str()),
        Some("responses")
    );
    assert_eq!(
        json.get("sum_key_header").and_then(|v| v.as_str()),
        Some("Authorization")
    );
}
