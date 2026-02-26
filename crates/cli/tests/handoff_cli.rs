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

fn make_hail_jsonl_with_cwd(session_id: &str, cwd: &Path) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("wire session sync"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();
    session.to_jsonl().expect("to jsonl")
}

fn make_hail_jsonl_with_cwd_and_window(
    session_id: &str,
    cwd: &Path,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> String {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.context.created_at = created_at;
    session.context.updated_at = updated_at;
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::UserMessage,
        task_id: None,
        content: Content::text("session spans multiple commits"),
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

fn setup_review_fixture(
    tmp: &tempfile::TempDir,
    fetch_hidden_refs: bool,
) -> (std::path::PathBuf, String) {
    let author = tmp.path().join("author");
    let reviewer = tmp.path().join("reviewer");
    let remote = tmp.path().join("review-remote.git");

    init_git_repo(&author);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote path")],
    );
    run_git(
        &author,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    run_git(&author, &["push", "origin", "main:main"]);

    run_git(&author, &["checkout", "-b", "feature/review"]);
    write_file(&author.join("src").join("feature.txt"), "review data\n");
    run_git(&author, &["add", "."]);
    run_git(&author, &["commit", "-m", "feat: add review flow"]);
    let feature_sha = first_non_empty_line(&run_git(&author, &["rev-parse", "HEAD"]).stdout);

    let storage = opensession_git_native::NativeGitStorage;
    let ledger_ref = opensession_git_native::branch_ledger_ref("feature/review");
    let session_body = make_hail_jsonl("s-review");
    let meta_body = serde_json::json!({
        "schema_version": 2,
        "session_id": "s-review",
        "git": { "commits": [feature_sha.clone()] }
    })
    .to_string();
    storage
        .store_session_at_ref(
            &author,
            &ledger_ref,
            "s-review",
            session_body.as_bytes(),
            meta_body.as_bytes(),
            std::slice::from_ref(&feature_sha),
        )
        .expect("store session in hidden ledger");

    run_git(
        &author,
        &["push", "origin", &format!("{feature_sha}:refs/pull/7/head")],
    );
    run_git(
        &author,
        &["push", "origin", &format!("{ledger_ref}:{ledger_ref}")],
    );

    run_git(
        tmp.path(),
        &[
            "clone",
            remote.to_str().expect("remote path"),
            reviewer.to_str().expect("reviewer path"),
        ],
    );
    run_git(
        &reviewer,
        &[
            "fetch",
            "origin",
            "+refs/pull/7/head:refs/opensession/review/pr/7/head",
        ],
    );
    if fetch_hidden_refs {
        run_git(
            &reviewer,
            &[
                "fetch",
                "origin",
                "+refs/opensession/*:refs/remotes/origin/opensession/*",
            ],
        );
    }

    run_git(
        &reviewer,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/private-repo.git",
        ],
    );

    (
        reviewer,
        "https://github.com/acme/private-repo/pull/7".to_string(),
    )
}

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
            &format!(
                "{}:sessions/{hash}.jsonl",
                opensession_git_native::branch_ledger_ref("main")
            ),
        ],
    );
}

#[test]
fn share_git_with_gitlab_dot_com_remote_emits_gl_uri() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-gl"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &repo,
        &[
            "share",
            &local_uri,
            "--git",
            "--remote",
            "https://gitlab.com/group/sub/repo.git",
        ],
    );
    assert!(
        share_out.status.success(),
        "share --git failed: {}",
        String::from_utf8_lossy(&share_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&share_out.stdout);
    let shared_uri = stdout.lines().next().unwrap_or_default();
    assert!(
        shared_uri.starts_with("os://src/gl/"),
        "expected gl uri, got: {shared_uri}"
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
fn share_web_supports_gl_and_git_routes() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let init_out = run(
        tmp.path(),
        &repo,
        &["config", "init", "--base-url", "https://example.test"],
    );
    assert!(init_out.status.success());

    let gl_uri = opensession_core::source_uri::SourceUri::Src(
        opensession_core::source_uri::SourceSpec::Gl {
            project: "group/sub/repo".to_string(),
            r#ref: "refs/heads/main".to_string(),
            path: "sessions/demo.jsonl".to_string(),
        },
    )
    .to_string();
    let gl_out = run(tmp.path(), &repo, &["share", &gl_uri, "--web"]);
    assert!(
        gl_out.status.success(),
        "share web for gl failed: {}",
        String::from_utf8_lossy(&gl_out.stderr)
    );
    let gl_stdout = String::from_utf8_lossy(&gl_out.stdout);
    assert!(gl_stdout.contains("https://example.test/src/gl/"));

    let git_uri = opensession_core::source_uri::SourceUri::Src(
        opensession_core::source_uri::SourceSpec::Git {
            remote: "https://gitlab.internal.example.com/group/repo.git".to_string(),
            r#ref: "refs/heads/main".to_string(),
            path: "sessions/demo.jsonl".to_string(),
        },
    )
    .to_string();
    let git_out = run(tmp.path(), &repo, &["share", &git_uri, "--web"]);
    assert!(
        git_out.status.success(),
        "share web for git failed: {}",
        String::from_utf8_lossy(&git_out.stderr)
    );
    let git_stdout = String::from_utf8_lossy(&git_out.stdout);
    assert!(git_stdout.contains("https://example.test/src/git/"));
}

#[test]
fn view_web_maps_remote_source_uri_to_src_route() {
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
        opensession_core::source_uri::SourceSpec::Gl {
            project: "group/sub/repo".to_string(),
            r#ref: "refs/heads/main".to_string(),
            path: "sessions/demo.jsonl".to_string(),
        },
    )
    .to_string();
    let out = run(tmp.path(), &repo, &["view", &uri, "--no-open", "--json"]);
    assert!(
        out.status.success(),
        "view failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("view json");
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    assert!(
        url.starts_with("https://example.test/src/gl/"),
        "unexpected url: {url}"
    );
}

#[test]
fn view_local_uri_emits_local_review_url_without_opening_browser() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-view-local"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let view_out = run(
        tmp.path(),
        &repo,
        &["view", &local_uri, "--no-open", "--json"],
    );
    assert!(
        view_out.status.success(),
        "view failed: {}",
        String::from_utf8_lossy(&view_out.stderr)
    );
    let payload: Value = serde_json::from_slice(&view_out.stdout).expect("view json");
    assert_eq!(payload.get("mode").and_then(Value::as_str), Some("local"));
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(url.contains("/review/local/"), "unexpected url: {url}");
}

#[test]
fn view_commit_target_builds_commit_review_bundle() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["view", "HEAD", "--no-open", "--json"]);
    assert!(
        out.status.success(),
        "view commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("view json");
    assert_eq!(payload.get("mode").and_then(Value::as_str), Some("commit"));
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(url.contains("/review/local/"), "unexpected url: {url}");
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
fn setup_installs_pre_push_hook_with_backup() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    write_file(
        &repo.join(".git").join("hooks").join("pre-push"),
        "#!/bin/sh\necho custom\n",
    );

    let out = run(tmp.path(), &repo, &["setup"]);
    assert!(
        out.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let backup = repo
        .join(".git")
        .join("hooks")
        .join("pre-push.pre-opensession");
    assert!(backup.exists(), "expected backup hook");
    let shim = tmp
        .path()
        .join(".local")
        .join("share")
        .join("opensession")
        .join("bin")
        .join("opensession");
    assert!(shim.exists(), "expected setup to install opensession shim");
    let ops_shim = tmp
        .path()
        .join(".local")
        .join("share")
        .join("opensession")
        .join("bin")
        .join("ops");
    assert!(ops_shim.exists(), "expected setup to install ops shim");

    let hook_body = fs::read_to_string(repo.join(".git").join("hooks").join("pre-push"))
        .expect("read pre-push hook");
    assert!(hook_body.contains("opensession-managed"));
    assert!(hook_body.contains("setup --sync-branch-session"));
    assert!(hook_body.contains("--sync-branch-commit"));
    assert!(hook_body.contains("setup --print-ledger-ref"));
    assert!(hook_body.contains("setup --print-fanout-mode"));
    assert!(hook_body.contains("git notes --ref=opensession"));
    assert!(hook_body.contains("git notes --ref=opensession copy -f"));
    assert!(hook_body.contains(".local/share/opensession/bin/opensession"));
}

#[test]
fn setup_check_prints_expected_ledger_ref() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["setup", "--check"]);
    assert!(
        out.status.success(),
        "setup --check failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("current branch: main"));
    assert!(stdout.contains(&opensession_git_native::branch_ledger_ref("main")));
    assert!(stdout.contains("ops shim:"));
    assert!(stdout.contains("review readiness:"));
}

#[test]
fn setup_print_fanout_mode_reads_git_config() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["setup", "--print-fanout-mode"]);
    assert!(
        out.status.success(),
        "print-fanout-mode failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hidden_ref");

    let git_config = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("config")
        .arg("--local")
        .arg("opensession.fanout-mode")
        .arg("git_notes")
        .output()
        .expect("set git config");
    assert!(
        git_config.status.success(),
        "{}",
        String::from_utf8_lossy(&git_config.stderr)
    );

    let out = run(tmp.path(), &repo, &["setup", "--print-fanout-mode"]);
    assert!(
        out.status.success(),
        "print-fanout-mode failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "git_notes");
}

#[test]
fn setup_sync_branch_session_stores_latest_repo_session_to_hidden_ref() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let head_sha = first_non_empty_line(&run_git(&repo, &["rev-parse", "HEAD"]).stdout);
    let session_path = tmp
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("02")
        .join("26")
        .join("rollout-2026-02-26T00-00-00-sync-session-1.jsonl");
    write_file(
        &session_path,
        &make_hail_jsonl_with_cwd("sync-session-1", &repo),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "setup",
            "--sync-branch-session",
            "main",
            "--sync-branch-commit",
            &head_sha,
        ],
    );
    assert!(
        out.status.success(),
        "sync branch session failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ledger_ref = opensession_git_native::branch_ledger_ref("main");
    let verify = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("show-ref")
        .arg("--verify")
        .arg("--quiet")
        .arg(&ledger_ref)
        .output()
        .expect("verify ledger ref exists");
    assert!(
        verify.status.success(),
        "expected ledger ref to exist after sync"
    );

    let index_blob = run_git(
        &repo,
        &[
            "show",
            &format!("{ledger_ref}:v1/index/commits/{head_sha}/sync-session-1.json"),
        ],
    );
    let index_body = String::from_utf8_lossy(&index_blob.stdout);
    assert!(index_body.contains("\"session_id\":\"sync-session-1\""));
}

#[test]
fn setup_sync_branch_session_maps_single_session_to_multiple_commits() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    write_file(&repo.join("a.txt"), "a\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "feat: a"]);
    let commit_a = first_non_empty_line(&run_git(&repo, &["rev-parse", "HEAD"]).stdout);

    write_file(&repo.join("b.txt"), "b\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "feat: b"]);
    let commit_b = first_non_empty_line(&run_git(&repo, &["rev-parse", "HEAD"]).stdout);

    let session_path = tmp
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("02")
        .join("26")
        .join("rollout-2026-02-26T00-00-00-sync-session-multi.jsonl");
    let created = chrono::Utc::now() - chrono::Duration::hours(2);
    let updated = chrono::Utc::now() + chrono::Duration::hours(2);
    write_file(
        &session_path,
        &make_hail_jsonl_with_cwd_and_window("sync-session-multi", &repo, created, updated),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "setup",
            "--sync-branch-session",
            "main",
            "--sync-branch-commit",
            &commit_b,
        ],
    );
    assert!(
        out.status.success(),
        "sync branch session failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let ledger_ref = opensession_git_native::branch_ledger_ref("main");
    run_git(
        &repo,
        &[
            "show",
            &format!("{ledger_ref}:v1/index/commits/{commit_a}/sync-session-multi.json"),
        ],
    );
    run_git(
        &repo,
        &[
            "show",
            &format!("{ledger_ref}:v1/index/commits/{commit_b}/sync-session-multi.json"),
        ],
    );
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
fn review_json_builds_commit_grouped_bundle_from_hidden_refs() {
    let tmp = make_home();
    let (reviewer_repo, pr_link) = setup_review_fixture(&tmp, true);

    let out = run(
        tmp.path(),
        &reviewer_repo,
        &["review", &pr_link, "--json", "--no-fetch"],
    );
    assert!(
        out.status.success(),
        "review failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let payload: Value = serde_json::from_slice(&out.stdout).expect("review json payload");
    assert_eq!(payload["commit_count"].as_u64().unwrap_or(0), 1);
    assert_eq!(payload["mapped_commit_count"].as_u64().unwrap_or(0), 1);
    assert!(payload["session_count"].as_u64().unwrap_or(0) >= 1);

    let bundle_path = payload["bundle_path"]
        .as_str()
        .expect("bundle path in payload");
    let bundle_raw = fs::read(bundle_path).expect("read review bundle");
    let bundle_json: Value = serde_json::from_slice(&bundle_raw).expect("bundle json");
    let first_commit = bundle_json["commits"]
        .as_array()
        .and_then(|rows| rows.first())
        .expect("first commit row");
    let first_session = first_commit["session_ids"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(first_session, "s-review");
}

#[test]
fn review_no_fetch_succeeds_with_empty_session_groups_when_hidden_refs_missing() {
    let tmp = make_home();
    let (reviewer_repo, pr_link) = setup_review_fixture(&tmp, false);

    let out = run(
        tmp.path(),
        &reviewer_repo,
        &["review", &pr_link, "--json", "--no-fetch"],
    );
    assert!(
        out.status.success(),
        "review should succeed without hidden refs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("review json payload");
    assert_eq!(payload["commit_count"].as_u64().unwrap_or(0), 1);
    assert_eq!(payload["mapped_commit_count"].as_u64().unwrap_or(0), 0);
    assert_eq!(payload["session_count"].as_u64().unwrap_or(0), 0);
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
