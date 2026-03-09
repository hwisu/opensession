use super::*;

#[test]
fn setup_non_tty_requires_yes() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["setup"]);
    assert!(
        !out.status.success(),
        "setup should require --yes in non-tty mode"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("requires explicit approval"));
    assert!(
        stderr.contains("opensession doctor --fix --yes --profile local --fanout-mode hidden_ref")
    );
}

#[test]
fn setup_non_tty_yes_requires_explicit_fanout_when_unset() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["setup", "--yes"]);
    assert!(
        !out.status.success(),
        "setup --yes should fail without explicit fanout when repo has no fanout config"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fanout mode is not configured"));
    assert!(
        stderr.contains("opensession doctor --fix --yes --profile local --fanout-mode hidden_ref")
    );
}

#[test]
fn setup_yes_with_fanout_installs_pre_push_hook_with_original_copy() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    write_file(
        &repo.join(".git").join("hooks").join("pre-push"),
        "#!/bin/sh\necho custom\n",
    );

    let out = run(
        tmp.path(),
        &repo,
        &["setup", "--yes", "--fanout-mode", "hidden_ref"],
    );
    assert!(
        out.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let backup = repo
        .join(".git")
        .join("hooks")
        .join("pre-push.original.pre-opensession");
    assert!(backup.exists(), "expected original hook copy");
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

    let fanout = run_git(
        &repo,
        &["config", "--local", "--get", "opensession.fanout-mode"],
    );
    assert_eq!(String::from_utf8_lossy(&fanout.stdout).trim(), "hidden_ref");
    let open_target = run_git(
        &repo,
        &["config", "--local", "--get", "opensession.open-target"],
    );
    assert_eq!(String::from_utf8_lossy(&open_target.stdout).trim(), "web");
}

#[test]
fn doctor_fix_yes_with_fanout_mode_applies_setup() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(
        tmp.path(),
        &repo,
        &[
            "doctor",
            "--fix",
            "--yes",
            "--fanout-mode",
            "git_notes",
            "--open-target",
            "web",
        ],
    );
    assert!(
        out.status.success(),
        "doctor --fix failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let fanout = run_git(
        &repo,
        &["config", "--local", "--get", "opensession.fanout-mode"],
    );
    assert_eq!(String::from_utf8_lossy(&fanout.stdout).trim(), "git_notes");
    let open_target = run_git(
        &repo,
        &["config", "--local", "--get", "opensession.open-target"],
    );
    assert_eq!(String::from_utf8_lossy(&open_target.stdout).trim(), "web");
    assert!(
        repo.join(".git").join("hooks").join("pre-push").exists(),
        "expected pre-push hook to be installed by doctor --fix"
    );
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
    assert!(stdout.contains("current branch:"));
    assert!(stdout.contains("main"));
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
fn setup_sync_branch_session_skips_auxiliary_sessions() {
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
        .join("rollout-2026-02-26T00-00-00-sync-session-aux.jsonl");
    write_file(
        &session_path,
        &make_auxiliary_hail_jsonl_with_cwd("sync-session-aux", &repo, "parent-session"),
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
        .expect("verify ledger ref absence");
    assert!(
        !verify.status.success(),
        "auxiliary sessions should not create a hidden ledger ref"
    );
}
