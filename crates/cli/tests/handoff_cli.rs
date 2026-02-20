use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    let body = r#"{"type":"user","uuid":"u1","sessionId":"handoff-cli-test","timestamp":"2026-02-14T00:00:01Z","message":{"role":"user","content":"fix handoff command"}}
{"type":"assistant","uuid":"a1","sessionId":"handoff-cli-test","timestamp":"2026-02-14T00:00:02Z","message":{"role":"assistant","model":"gpt-4.1","content":[{"type":"text","text":"I will update it."}]}}"#;
    write_file(&path, body);
    path
}

fn create_codex_assistant_only_session(home: &Path, rel: &str) -> PathBuf {
    let path = home.join(".codex").join("sessions").join(rel);
    let body = r#"{"type":"assistant","uuid":"a1","sessionId":"handoff-cli-test","timestamp":"2026-02-14T00:00:02Z","message":{"role":"assistant","model":"gpt-4.1","content":[{"type":"text","text":"assistant only output"}]}}"#;
    write_file(&path, body);
    path
}

fn run(home: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.args(args).env("HOME", home).env("NO_COLOR", "1");
    cmd.output().expect("run opensession")
}

fn run_in(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("NO_COLOR", "1");
    cmd.output().expect("run opensession")
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed\nstdout: {}\nstderr: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo(cwd: &Path) {
    fs::create_dir_all(cwd).expect("create repo dir");
    run_git(cwd, &["init", "--initial-branch=main"]);
    run_git(cwd, &["config", "user.email", "test@example.com"]);
    run_git(cwd, &["config", "user.name", "Test"]);
    write_file(&cwd.join("README.md"), "repo");
    run_git(cwd, &["add", "."]);
    run_git(cwd, &["commit", "-m", "init"]);
}

#[test]
fn top_help_hides_removed_commands() {
    let tmp = make_home();
    let output = run(tmp.path(), &["--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(!stdout.contains("\n  ui"));
    assert!(!stdout.contains("\n  view"));
    assert!(!stdout.contains("discover"));
    assert!(!stdout.contains("timeline"));
    assert!(!stdout.contains("\n  ops"));
    assert!(stdout.contains("\n  daemon"));
}

#[test]
fn handoff_help_hides_llm_flags() {
    let tmp = make_home();
    let output = run(tmp.path(), &["session", "handoff", "--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("--summarize"));
    assert!(!stdout.contains("--ai"));
    assert!(!stdout.contains("--legacy-schema"));
}

#[test]
fn handoff_last_supports_all_output_formats() {
    let tmp = make_home();
    let home = tmp.path();
    create_codex_session(home, "2026/02/14/handoff-cli-test.jsonl");

    for format in ["text", "markdown", "json", "jsonl", "hail", "stream"] {
        let output = run(home, &["session", "handoff", "--last", "--format", format]);
        assert!(
            output.status.success(),
            "format {format} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        match format {
            "text" | "markdown" => {
                assert!(stdout.contains("Session Handoff"));
            }
            "json" => {
                let parsed: Value = serde_json::from_str(&stdout).expect("json output");
                let arr = parsed.as_array().expect("json array");
                assert_eq!(arr.len(), 1);
            }
            "jsonl" => {
                let first = stdout.lines().next().expect("jsonl line");
                let parsed: Value = serde_json::from_str(first).expect("jsonl object");
                assert!(parsed.get("session_id").is_some());
            }
            "hail" => {
                assert!(stdout.contains("hail-1.0.0"));
            }
            "stream" => {
                let first = stdout.lines().next().expect("stream line");
                let parsed: Value = serde_json::from_str(first).expect("stream object");
                assert_eq!(
                    parsed.get("type").and_then(|v| v.as_str()),
                    Some("session_summary")
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn handoff_last_with_count_returns_multiple_sessions() {
    let tmp = make_home();
    let home = tmp.path();
    create_codex_session(home, "2026/02/14/handoff-last-a.jsonl");
    create_codex_session(home, "2026/02/15/handoff-last-b.jsonl");

    let output = run(
        home,
        &["session", "handoff", "--last", "2", "--format", "json"],
    );
    assert!(
        output.status.success(),
        "handoff --last 2 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json output");
    let arr = parsed.as_array().expect("json array");
    assert_eq!(arr.len(), 2);
}

#[test]
fn handoff_defaults_to_json_and_last_when_piped() {
    let tmp = make_home();
    let home = tmp.path();
    create_codex_session(home, "2026/02/14/handoff-default-pipe.jsonl");

    // Command output is captured in tests (non-tty), so default should be JSON,
    // and missing explicit session ref should auto-fallback to latest.
    let output = run(home, &["session", "handoff"]);
    assert!(
        output.status.success(),
        "handoff default failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("default piped output is json");
    let arr = parsed.as_array().expect("json array");
    assert_eq!(arr.len(), 1);
    assert!(arr[0].get("session_id").is_some());
}

#[test]
fn handoff_validate_reports_but_exits_zero() {
    let tmp = make_home();
    let home = tmp.path();
    let session = create_codex_assistant_only_session(home, "2026/02/14/handoff-validate.jsonl");

    let output = run(
        home,
        &[
            "session",
            "handoff",
            session.to_str().expect("session path"),
            "--format",
            "json",
            "--validate",
        ],
    );
    assert!(
        output.status.success(),
        "validate should be soft-pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Handoff validation:"));
    assert!(stderr.contains("\"type\":\"handoff_validation\""));
}

#[test]
fn handoff_strict_does_not_fail_on_warning_only_findings() {
    let tmp = make_home();
    let home = tmp.path();
    let session = create_codex_assistant_only_session(home, "2026/02/14/handoff-strict.jsonl");

    let output = run(
        home,
        &[
            "session",
            "handoff",
            session.to_str().expect("session path"),
            "--strict",
        ],
    );
    assert!(
        output.status.success(),
        "strict should pass when findings are warning-only, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("\"type\":\"handoff_validation\""));
}

#[test]
fn handoff_json_shape_is_v2() {
    let tmp = make_home();
    let home = tmp.path();
    let session = create_codex_session(home, "2026/02/14/handoff-v2-shape.jsonl");

    let output_v2 = run(
        home,
        &[
            "session",
            "handoff",
            session.to_str().expect("session path"),
            "--format",
            "json",
        ],
    );
    assert!(
        output_v2.status.success(),
        "v2 handoff failed: {}",
        String::from_utf8_lossy(&output_v2.stderr)
    );
    let parsed_v2: Value = serde_json::from_slice(&output_v2.stdout).expect("v2 json output");
    let arr_v2 = parsed_v2.as_array().expect("v2 json array");
    assert_eq!(arr_v2.len(), 1);
    let v2 = &arr_v2[0];
    assert!(v2.get("execution_contract").is_some());
    assert!(v2.get("task_summaries").is_none());
    assert!(v2.get("errors").is_none());
    assert!(v2.get("shell_commands").is_none());
}

#[test]
fn handoff_artifact_save_list_show_refresh_render_md() {
    let tmp = make_home();
    let home = tmp.path();
    let repo = home.join("repo");
    init_git_repo(&repo);

    let session_a = create_codex_session(home, "2026/02/14/handoff-artifact-a.jsonl");
    let session_b = create_codex_session(home, "2026/02/15/handoff-artifact-b.jsonl");

    let save_output = run_in(
        home,
        &repo,
        &[
            "session",
            "handoff",
            "save",
            session_a.to_str().expect("session path"),
            session_b.to_str().expect("session path"),
            "--payload-format",
            "jsonl",
        ],
    );
    assert!(
        save_output.status.success(),
        "save failed: {}",
        String::from_utf8_lossy(&save_output.stderr)
    );
    let save_json: Value = serde_json::from_slice(&save_output.stdout).expect("save json");
    let artifact_id = save_json
        .get("artifact_id")
        .and_then(|value| value.as_str())
        .expect("artifact_id")
        .to_string();

    let list_output = run_in(home, &repo, &["session", "handoff", "artifact", "list"]);
    assert!(
        list_output.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let list_json: Value = serde_json::from_slice(&list_output.stdout).expect("list json");
    let list = list_json.as_array().expect("list array");
    assert!(list.iter().any(|row| {
        row.get("artifact_id").and_then(|value| value.as_str()) == Some(artifact_id.as_str())
    }));

    let show_output = run_in(
        home,
        &repo,
        &["session", "handoff", "artifact", "show", &artifact_id],
    );
    assert!(
        show_output.status.success(),
        "show failed: {}",
        String::from_utf8_lossy(&show_output.stderr)
    );
    let show_json: Value = serde_json::from_slice(&show_output.stdout).expect("show json");
    assert_eq!(
        show_json.get("stale").and_then(|value| value.as_bool()),
        Some(false)
    );

    let changed_body = r#"{"type":"user","uuid":"u1","sessionId":"handoff-cli-test","timestamp":"2026-02-15T00:00:01Z","message":{"role":"user","content":"refresh this"}}
{"type":"assistant","uuid":"a1","sessionId":"handoff-cli-test","timestamp":"2026-02-15T00:00:02Z","message":{"role":"assistant","model":"gpt-4.1","content":[{"type":"text","text":"updated output with extra content"}]}}"#;
    write_file(&session_b, changed_body);

    let stale_output = run_in(
        home,
        &repo,
        &["session", "handoff", "artifact", "show", &artifact_id],
    );
    assert!(
        stale_output.status.success(),
        "show stale failed: {}",
        String::from_utf8_lossy(&stale_output.stderr)
    );
    let stale_json: Value = serde_json::from_slice(&stale_output.stdout).expect("show stale json");
    assert_eq!(
        stale_json.get("stale").and_then(|value| value.as_bool()),
        Some(true)
    );

    let refresh_output = run_in(
        home,
        &repo,
        &["session", "handoff", "artifact", "refresh", &artifact_id],
    );
    assert!(
        refresh_output.status.success(),
        "refresh failed: {}",
        String::from_utf8_lossy(&refresh_output.stderr)
    );
    let refresh_json: Value = serde_json::from_slice(&refresh_output.stdout).expect("refresh json");
    assert_eq!(
        refresh_json
            .get("refreshed")
            .and_then(|value| value.as_bool()),
        Some(true)
    );

    let out_md = repo.join("HANDOFF.generated.md");
    let render_output = run_in(
        home,
        &repo,
        &[
            "session",
            "handoff",
            "artifact",
            "render-md",
            &artifact_id,
            "--output",
            out_md.to_str().expect("md path"),
        ],
    );
    assert!(
        render_output.status.success(),
        "render-md failed: {}",
        String::from_utf8_lossy(&render_output.stderr)
    );
    let markdown = fs::read_to_string(&out_md).expect("rendered markdown");
    assert!(markdown.contains("Session Handoff") || markdown.contains("Merged Session Handoff"));
}
