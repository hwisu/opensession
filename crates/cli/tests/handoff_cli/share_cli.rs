use super::*;
use opensession_core::ContentBlock;

fn init_share_repo(home: &Path, repo: &Path) {
    let out = run(
        home,
        repo,
        &["config", "init", "--base-url", "https://example.test"],
    );
    assert!(
        out.status.success(),
        "config init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
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
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("--git --remote"));
}

#[test]
fn share_git_requires_remote_guidance() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-missing-remote"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(tmp.path(), &repo, &["share", &local_uri, "--git"]);
    assert!(!share_out.status.success());
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("`--remote <name|url>` is required"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession share <local_uri> --git --remote origin"));
}

#[test]
fn share_quick_auto_detects_origin_without_push_and_reports_state() {
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
    init_share_repo(tmp.path(), &repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-quick-no-push", &repo),
    );
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &repo,
        &["share", &local_uri, "--quick", "--json"],
    );
    assert!(
        share_out.status.success(),
        "share --quick failed: {}",
        String::from_utf8_lossy(&share_out.stderr)
    );
    let payload: Value = serde_json::from_slice(&share_out.stdout).expect("quick share json");
    assert_eq!(payload.get("quick").and_then(Value::as_bool), Some(true));
    assert_eq!(payload.get("pushed").and_then(Value::as_bool), Some(false));
    assert_eq!(
        payload.get("auto_push_consent").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        payload.get("remote_target").and_then(Value::as_str),
        Some("origin")
    );
    assert!(
        payload
            .get("push_cmd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("git push origin")
    );
}

#[test]
fn share_quick_push_consent_persists_and_enables_auto_push() {
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
    init_share_repo(tmp.path(), &repo);

    let first_input = repo.join("first.hail.jsonl");
    write_file(
        &first_input,
        &make_hail_jsonl_with_cwd("s-share-quick-first", &repo),
    );
    let first_register = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", first_input.to_str().expect("path")],
    );
    let first_local_uri = first_non_empty_line(&first_register.stdout);
    let first_share = run(
        tmp.path(),
        &repo,
        &["share", &first_local_uri, "--quick", "--push", "--json"],
    );
    assert!(
        first_share.status.success(),
        "share --quick --push failed: {}",
        String::from_utf8_lossy(&first_share.stderr)
    );
    let first_payload: Value =
        serde_json::from_slice(&first_share.stdout).expect("quick share json");
    assert_eq!(
        first_payload.get("quick").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        first_payload.get("pushed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        first_payload
            .get("auto_push_consent")
            .and_then(Value::as_bool),
        Some(true)
    );

    let consent_config = first_non_empty_line(
        &run_git(
            &repo,
            &[
                "config",
                "--local",
                "--get",
                "opensession.share.auto-push-consent",
            ],
        )
        .stdout,
    );
    assert_eq!(consent_config, "true");

    let second_input = repo.join("second.hail.jsonl");
    write_file(
        &second_input,
        &make_hail_jsonl_with_cwd("s-share-quick-second", &repo),
    );
    let second_register = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", second_input.to_str().expect("path")],
    );
    let second_local_uri = first_non_empty_line(&second_register.stdout);
    let second_hash = second_local_uri
        .split('/')
        .next_back()
        .expect("local uri hash")
        .to_string();

    let second_share = run(
        tmp.path(),
        &repo,
        &["share", &second_local_uri, "--quick", "--json"],
    );
    assert!(
        second_share.status.success(),
        "second share --quick failed: {}",
        String::from_utf8_lossy(&second_share.stderr)
    );
    let second_payload: Value =
        serde_json::from_slice(&second_share.stdout).expect("second quick share json");
    assert_eq!(
        second_payload.get("quick").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second_payload.get("pushed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second_payload
            .get("auto_push_consent")
            .and_then(Value::as_bool),
        Some(true)
    );

    let ledger_ref = opensession_git_native::branch_ledger_ref("main");
    run_git(
        tmp.path(),
        &[
            "--git-dir",
            remote.to_str().expect("remote"),
            "show",
            &format!("{ledger_ref}:sessions/{second_hash}.jsonl"),
        ],
    );
}

#[test]
fn share_quick_requires_remote_when_none_exist() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-quick-no-remote"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(tmp.path(), &repo, &["share", &local_uri, "--quick"]);
    assert!(!share_out.status.success());
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("no remotes were found"));
    assert!(stderr.contains("git remote add origin"));
}

#[test]
fn share_quick_rejects_ambiguous_remote_and_allows_explicit_override() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);
    run_git(
        &repo,
        &[
            "remote",
            "add",
            "upstream",
            "https://github.com/example/upstream.git",
        ],
    );
    run_git(
        &repo,
        &[
            "remote",
            "add",
            "mirror",
            "https://github.com/example/mirror.git",
        ],
    );
    init_share_repo(tmp.path(), &repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-quick-ambiguous", &repo),
    );
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let ambiguous = run(tmp.path(), &repo, &["share", &local_uri, "--quick"]);
    assert!(!ambiguous.status.success());
    let stderr = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(stderr.contains("could not choose a remote automatically"));
    assert!(stderr.contains("--quick --remote origin"));

    let explicit = run(
        tmp.path(),
        &repo,
        &[
            "share", &local_uri, "--quick", "--remote", "upstream", "--json",
        ],
    );
    assert!(
        explicit.status.success(),
        "share --quick --remote upstream failed: {}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    let payload: Value =
        serde_json::from_slice(&explicit.stdout).expect("explicit quick share json");
    assert_eq!(payload.get("quick").and_then(Value::as_bool), Some(true));
    assert_eq!(
        payload.get("remote_target").and_then(Value::as_str),
        Some("upstream")
    );
}

#[test]
fn share_web_requires_config_with_next_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let uri = opensession_core::source_uri::SourceUri::Src(
        opensession_core::source_uri::SourceSpec::Git {
            remote: "https://git.example/repo.git".to_string(),
            r#ref: "refs/heads/main".to_string(),
            path: "sessions/demo.jsonl".to_string(),
        },
    )
    .to_string();

    let out = run(tmp.path(), &repo, &["share", &uri, "--web"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing config"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession config init --base-url"));
}

#[test]
fn share_git_outside_repo_shows_next_steps() {
    let tmp = make_home();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).expect("create outside dir");

    let input = outside.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl("s-share-outside"));
    let register_out = run(
        tmp.path(),
        &outside,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);
    assert!(local_uri.starts_with("os://src/local/"));

    let share_out = run(
        tmp.path(),
        &outside,
        &["share", &local_uri, "--git", "--remote", "origin"],
    );
    assert!(!share_out.status.success());
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("current directory is not inside a git repository"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("cd into the target git repository and retry"));
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
    init_share_repo(tmp.path(), &repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl_with_cwd("s-share-git", &repo));
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
    init_share_repo(tmp.path(), &repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(&input, &make_hail_jsonl_with_cwd("s-share-gl", &repo));
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
fn share_git_push_in_non_tty_does_not_trigger_cleanup_prompt() {
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
    init_share_repo(tmp.path(), &repo);

    let input = repo.join("sample.hail.jsonl");
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-push-no-tty", &repo),
    );
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &repo,
        &["share", &local_uri, "--git", "--remote", "origin", "--push"],
    );
    assert!(
        share_out.status.success(),
        "share --push failed: {}",
        String::from_utf8_lossy(&share_out.stderr)
    );

    assert!(
        !repo
            .join(".opensession")
            .join("cleanup")
            .join("config.toml")
            .exists(),
        "non-tty share should not auto-initialize cleanup config"
    );

    let prompted = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("config")
        .arg("--local")
        .arg("--get")
        .arg("opensession.cleanup.prompted")
        .output()
        .expect("read prompted config");
    assert!(
        !prompted.status.success(),
        "non-tty share should not set cleanup prompt git config"
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
fn share_git_blocks_public_share_until_repo_is_initialized() {
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
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-uninitialized", &repo),
    );
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
        !share_out.status.success(),
        "share unexpectedly succeeded without repo init"
    );
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("explicitly initialized for OpenSession"));
    assert!(stderr.contains("opensession doctor --fix"));
    assert!(stderr.contains("opensession config init"));
}

#[test]
fn share_git_blocks_sessions_from_a_different_repo() {
    let tmp = make_home();
    let source_repo = tmp.path().join("source-repo");
    let share_repo = tmp.path().join("share-repo");
    let remote = tmp.path().join("remote.git");
    let outside = tmp.path().join("outside");
    init_git_repo(&source_repo);
    init_git_repo(&share_repo);
    fs::create_dir_all(&outside).expect("create outside dir");
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &share_repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    init_share_repo(tmp.path(), &share_repo);

    let input = outside.join("cross-repo.hail.jsonl");
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-cross-repo", &source_repo),
    );
    let register_out = run(
        tmp.path(),
        &outside,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &share_repo,
        &["share", &local_uri, "--git", "--remote", "origin"],
    );
    assert!(
        !share_out.status.success(),
        "share unexpectedly succeeded for cross-repo session"
    );
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("originated from a different git repository"));
    assert!(stderr.contains(source_repo.to_str().expect("source repo path")));
    assert!(stderr.contains(share_repo.to_str().expect("share repo path")));
}

#[test]
fn share_git_blocks_sessions_recorded_outside_any_repo() {
    let tmp = make_home();
    let share_repo = tmp.path().join("share-repo");
    let remote = tmp.path().join("remote.git");
    let outside = tmp.path().join("outside-work");
    init_git_repo(&share_repo);
    fs::create_dir_all(&outside).expect("create outside work dir");
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &share_repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    init_share_repo(tmp.path(), &share_repo);

    let input = outside.join("outside.hail.jsonl");
    write_file(
        &input,
        &make_hail_jsonl_with_cwd("s-share-outside-origin", &outside),
    );
    let register_out = run(
        tmp.path(),
        &outside,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);

    let share_out = run(
        tmp.path(),
        &share_repo,
        &["share", &local_uri, "--git", "--remote", "origin"],
    );
    assert!(
        !share_out.status.success(),
        "share unexpectedly succeeded for non-repo session"
    );
    let stderr = String::from_utf8_lossy(&share_out.stderr);
    assert!(stderr.contains("was not recorded inside a git repository"));
    assert!(stderr.contains("keep this session local-only"));
}

#[test]
fn share_git_sanitizes_sensitive_paths_and_credentials_before_writing_git() {
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
    init_share_repo(tmp.path(), &repo);

    let home_sensitive = tmp.path().join(".zshrc");
    let mut session = Session::new(
        "s-share-sensitive".to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.context.title = Some(format!("review {}", home_sensitive.display()));
    session
        .context
        .attributes
        .insert("cwd".to_string(), Value::String(repo.display().to_string()));
    session.context.attributes.insert(
        "source_path".to_string(),
        Value::String(home_sensitive.display().to_string()),
    );
    session.events.push(Event {
        event_id: "e1".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::FileEdit {
            path: home_sensitive.display().to_string(),
            diff: Some("+ export API_KEY=sk-sensitive".to_string()),
        },
        task_id: None,
        content: Content {
            blocks: vec![
                ContentBlock::Text {
                    text: format!("cat {}", home_sensitive.display()),
                },
                ContentBlock::File {
                    path: home_sensitive.display().to_string(),
                    content: Some("export API_KEY=sk-sensitive".to_string()),
                },
            ],
        },
        duration_ms: None,
        attributes: Default::default(),
    });
    session.events.push(Event {
        event_id: "e2".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::ShellCommand {
            command: format!("cat {} API_KEY=sk-sensitive", home_sensitive.display()),
            exit_code: Some(0),
        },
        task_id: None,
        content: Content::text("done"),
        duration_ms: None,
        attributes: Default::default(),
    });
    session.recompute_stats();

    let input = repo.join("sensitive.hail.jsonl");
    write_file(&input, &session.to_jsonl().expect("session jsonl"));
    let register_out = run(
        tmp.path(),
        &repo,
        &["register", "--quiet", input.to_str().expect("path")],
    );
    let local_uri = first_non_empty_line(&register_out.stdout);
    let local_hash = local_uri
        .split('/')
        .next_back()
        .expect("local hash")
        .to_string();

    let share_out = run(
        tmp.path(),
        &repo,
        &["share", &local_uri, "--git", "--remote", "origin"],
    );
    assert!(
        share_out.status.success(),
        "share failed: {}",
        String::from_utf8_lossy(&share_out.stderr)
    );

    let stored = run_git(
        &repo,
        &[
            "show",
            &format!(
                "{}:sessions/{local_hash}.jsonl",
                opensession_git_native::branch_ledger_ref("main")
            ),
        ],
    );
    let shared_body = String::from_utf8_lossy(&stored.stdout);
    assert!(!shared_body.contains("sk-sensitive"), "{shared_body}");
    assert!(!shared_body.contains(".zshrc"));
    assert!(!shared_body.contains(&home_sensitive.display().to_string()));
    assert!(shared_body.contains("[REDACTED_CREDENTIAL]"));
    assert!(shared_body.contains("[REDACTED_SENSITIVE_PATH]"));
    assert!(shared_body.contains("[REDACTED_SENSITIVE_FILE]"));
}
