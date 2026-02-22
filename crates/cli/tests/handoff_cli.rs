use opensession_core::testing;
use opensession_core::{Agent, Content, Event, EventType, Session};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn make_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, body).expect("write file");
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    cmd.args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("NO_COLOR", "1");
    cmd.output().expect("run opensession")
}

fn run_git(cwd: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {} failed\nstdout:{}\nstderr:{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn init_git_repo(path: &Path) {
    fs::create_dir_all(path).expect("create repo");
    run_git(path, &["init", "--initial-branch=main"]);
    run_git(path, &["config", "user.email", "test@example.com"]);
    run_git(path, &["config", "user.name", "Test"]);
    write_file(&path.join("README.md"), "repo\n");
    run_git(path, &["add", "."]);
    run_git(path, &["commit", "-m", "init"]);
}

fn make_hail_jsonl(session_id: &str) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("implement the feature"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn first_non_empty_line(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn help_shows_v1_commands() {
    let tmp = make_home();
    let output = run(tmp.path(), tmp.path(), &["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(stdout.contains("register"));
    assert!(stdout.contains("share"));
    assert!(stdout.contains("handoff"));
    assert!(!stdout.contains("publish"));
}

#[test]
fn register_and_cat_roundtrip() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-register"));

    let register_out = run(
        tmp.path(),
        &repo,
        &["register", input.to_str().expect("path")],
    );
    assert!(
        register_out.status.success(),
        "register failed: {}",
        String::from_utf8_lossy(&register_out.stderr)
    );
    let uri = first_non_empty_line(&register_out.stdout);
    assert!(uri.starts_with("os://src/local/"));

    let cat_out = run(tmp.path(), &repo, &["cat", &uri]);
    assert!(cat_out.status.success());
    let cat_body = String::from_utf8_lossy(&cat_out.stdout);
    let parsed = Session::from_jsonl(&cat_body).expect("cat output is valid jsonl");
    assert_eq!(parsed.session_id, "s-register");
}

#[test]
fn share_web_rejects_local_uri() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-web"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let output = run(tmp.path(), &repo, &["share", &local_uri, "--web"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--git --remote"));
}

#[test]
fn share_git_without_push_prints_push_command() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("remote.git");
    init_git_repo(&repo);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-git"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &repo,
        &["share", &local_uri, "--git", "--remote", "origin"],
    );
    assert!(
        share_out.status.success(),
        "share --git failed: {}",
        String::from_utf8_lossy(&share_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&share_out.stdout);
    let shared_uri = stdout.lines().next().unwrap_or_default();
    assert!(shared_uri.starts_with("os://src/git/") || shared_uri.starts_with("os://src/gh/"));
    assert!(stdout.contains("push_cmd:"));

    let hash = local_uri.split('/').next_back().expect("local hash in uri");

    run_git(
        &repo,
        &[
            "show",
            &format!("refs/heads/opensession/sessions:sessions/{hash}.jsonl"),
        ],
    );
}

#[test]
fn config_and_share_web_success() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let init_out = run(
        tmp.path(),
        &repo,
        &["config", "init", "--base-url", "https://example.test"],
    );
    assert!(init_out.status.success());

    let uri = opensession_core::source_uri::SourceUri::Src(
        opensession_core::source_uri::SourceSpec::Git {
            remote: "https://git.example/repo.git".to_string(),
            r#ref: "refs/heads/main".to_string(),
            path: "sessions/demo.jsonl".to_string(),
        },
    )
    .to_string();
    let out = run(tmp.path(), &repo, &["share", &uri, "--web"]);
    assert!(
        out.status.success(),
        "share web failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("https://example.test/src/git/"));
    assert!(stdout.contains("base_url: https://example.test"));
}

#[test]
fn handoff_build_get_verify_pin_unpin_rm() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input_a = repo.join("a.hail.jsonl");
    let input_b = repo.join("b.hail.jsonl");
    write_file(&input_a, &make_hail_jsonl("s-a"));
    write_file(&input_b, &make_hail_jsonl("s-b"));

    let reg_a = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input_a.to_str().expect("path")],
    );
    let reg_b = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input_b.to_str().expect("path")],
    );
    let uri_a = first_non_empty_line(&reg_a.stdout);
    let uri_b = first_non_empty_line(&reg_b.stdout);

    let build = run(
        tmp.path(),
        &repo,
        &[
            "handoff",
            "build",
            "--from",
            &uri_a,
            "--from",
            &uri_b,
            "--pin",
            "latest",
            "--validate",
        ],
    );
    assert!(
        build.status.success(),
        "handoff build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let artifact_uri = first_non_empty_line(&build.stdout);
    assert!(artifact_uri.starts_with("os://artifact/"));

    let get_json = run(
        tmp.path(),
        &repo,
        &[
            "handoff",
            "artifacts",
            "get",
            &artifact_uri,
            "--format",
            "canonical",
            "--encode",
            "json",
        ],
    );
    assert!(get_json.status.success());
    let parsed: Value = serde_json::from_slice(&get_json.stdout).expect("json output");
    assert!(parsed.as_array().is_some());

    let verify = run(
        tmp.path(),
        &repo,
        &["handoff", "artifacts", "verify", &artifact_uri],
    );
    assert!(verify.status.success());

    let rm_pinned = run(
        tmp.path(),
        &repo,
        &["handoff", "artifacts", "rm", &artifact_uri],
    );
    assert!(!rm_pinned.status.success());

    let unpin = run(
        tmp.path(),
        &repo,
        &["handoff", "artifacts", "unpin", "latest"],
    );
    assert!(unpin.status.success());

    let rm = run(
        tmp.path(),
        &repo,
        &["handoff", "artifacts", "rm", &artifact_uri],
    );
    assert!(rm.status.success());
}

#[test]
fn parse_profile_codex_outputs_canonical_jsonl() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("codex.jsonl");
    write_file(
        &input,
        r#"{"type":"session_meta","session_id":"abc","timestamp":"2026-02-14T00:00:00Z"}
{"type":"response_item","timestamp":"2026-02-14T00:00:01Z","payload":{"type":"message","role":"user","content":"hello"}}"#,
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "parse",
            "--profile",
            "codex",
            input.to_str().expect("path"),
            "--validate",
        ],
    );
    assert!(
        out.status.success(),
        "parse failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let parsed = Session::from_jsonl(&String::from_utf8_lossy(&out.stdout)).expect("jsonl");
    assert_eq!(parsed.version, Session::CURRENT_VERSION);
    assert_eq!(parsed.agent.tool, "codex");
}

#[test]
fn inspect_local_and_artifact_json() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-inspect"));

    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let inspect_local = run(tmp.path(), &repo, &["inspect", &local_uri, "--json"]);
    assert!(inspect_local.status.success());
    let local_json: Value = serde_json::from_slice(&inspect_local.stdout).expect("inspect local");
    assert_eq!(local_json["uri"], local_uri);

    let build = run(
        tmp.path(),
        &repo,
        &["handoff", "build", "--from", &local_uri],
    );
    assert!(build.status.success());
    let artifact_uri = first_non_empty_line(&build.stdout);

    let inspect_artifact = run(tmp.path(), &repo, &["inspect", &artifact_uri, "--json"]);
    assert!(inspect_artifact.status.success());
    let artifact_json: Value =
        serde_json::from_slice(&inspect_artifact.stdout).expect("inspect artifact");
    assert_eq!(artifact_json["uri"], artifact_uri);
}

#[test]
fn parse_preview_option_prints_parser_used() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-preview"));

    let out = run(
        tmp.path(),
        &repo,
        &[
            "parse",
            "--profile",
            "hail",
            "--preview",
            input.to_str().expect("path"),
        ],
    );
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("parser_used:"));
}

#[test]
fn canonical_jsonl_register_rejects_non_hail_input() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("raw.jsonl");
    write_file(&input, "{\"type\":\"session_meta\"}\n");

    let out = run(
        tmp.path(),
        &repo,
        &["register", input.to_str().expect("path")],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("opensession parse"));
}

#[test]
fn docs_completion_still_available() {
    let tmp = make_home();
    let out = run(tmp.path(), tmp.path(), &["docs", "completion", "bash"]);
    assert!(out.status.success());
    assert!(!out.stdout.is_empty());
}

#[test]
fn handoff_get_raw_jsonl_outputs_session_json_rows() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-raw"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let build = run(
        tmp.path(),
        &repo,
        &["handoff", "build", "--from", &local_uri],
    );
    let artifact_uri = first_non_empty_line(&build.stdout);

    let get = run(
        tmp.path(),
        &repo,
        &[
            "handoff",
            "artifacts",
            "get",
            &artifact_uri,
            "--format",
            "raw",
            "--encode",
            "jsonl",
        ],
    );
    assert!(get.status.success());
    let first_line = String::from_utf8_lossy(&get.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let row: Value = serde_json::from_str(&first_line).expect("json row");
    assert!(row.get("session_id").is_some());
}

#[test]
fn testing_helper_agent_is_available() {
    // Keep one assertion using opensession_core::testing to ensure dev-dependency path stays valid.
    let agent = testing::agent();
    assert!(!agent.tool.is_empty());
}
