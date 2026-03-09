use super::*;

#[test]
fn cleanup_init_github_writes_expected_files() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("cleanup-github-remote.git");
    init_git_repo(&repo);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );

    let out = run(
        tmp.path(),
        &repo,
        &["cleanup", "init", "--provider", "github", "--yes", "--json"],
    );
    assert!(
        out.status.success(),
        "cleanup init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let payload: Value = serde_json::from_slice(&out.stdout).expect("cleanup init json");
    assert_eq!(
        payload.get("configured").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("provider").and_then(Value::as_str),
        Some("github")
    );

    assert!(
        repo.join(".opensession")
            .join("cleanup")
            .join("config.toml")
            .exists(),
        "expected cleanup config to exist"
    );
    assert!(
        repo.join(".opensession")
            .join("cleanup")
            .join("janitor.sh")
            .exists(),
        "expected janitor script to exist"
    );
    assert!(
        repo.join(".github")
            .join("workflows")
            .join("opensession-cleanup.yml")
            .exists(),
        "expected github workflow template to exist"
    );
    assert!(
        repo.join(".github")
            .join("workflows")
            .join("opensession-session-review.yml")
            .exists(),
        "expected github session review workflow template to exist"
    );
}

#[test]
fn cleanup_init_gitlab_without_marker_reports_manual_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("cleanup-gitlab-remote.git");
    init_git_repo(&repo);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );

    write_file(&repo.join(".gitlab-ci.yml"), "stages:\n  - test\n");

    let out = run(
        tmp.path(),
        &repo,
        &["cleanup", "init", "--provider", "gitlab", "--yes", "--json"],
    );
    assert!(
        out.status.success(),
        "cleanup init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let payload: Value = serde_json::from_slice(&out.stdout).expect("cleanup init json");
    let manual_steps = payload
        .get("manual_steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        !manual_steps.is_empty(),
        "expected manual steps for gitlab-ci"
    );

    assert!(
        repo.join(".gitlab")
            .join("opensession-cleanup.yml")
            .exists(),
        "expected gitlab cleanup template to exist"
    );
    assert!(
        repo.join(".gitlab")
            .join("opensession-session-review.yml")
            .exists(),
        "expected gitlab session review template to exist"
    );

    let gitlab_ci = fs::read_to_string(repo.join(".gitlab-ci.yml")).expect("read gitlab-ci");
    assert!(gitlab_ci.contains("stages:"));
    assert!(!gitlab_ci.contains("opensession-managed-cleanup"));
}

#[test]
fn cleanup_status_reports_not_configured_then_configured() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("cleanup-status-remote.git");
    init_git_repo(&repo);
    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );

    let status_before = run(tmp.path(), &repo, &["cleanup", "status", "--json"]);
    assert!(
        status_before.status.success(),
        "cleanup status failed: {}",
        String::from_utf8_lossy(&status_before.stderr)
    );
    let before_payload: Value =
        serde_json::from_slice(&status_before.stdout).expect("cleanup status json");
    assert_eq!(
        before_payload.get("configured").and_then(Value::as_bool),
        Some(false)
    );

    let init = run(
        tmp.path(),
        &repo,
        &["cleanup", "init", "--provider", "generic", "--yes"],
    );
    assert!(
        init.status.success(),
        "cleanup init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let status_after = run(tmp.path(), &repo, &["cleanup", "status", "--json"]);
    assert!(
        status_after.status.success(),
        "cleanup status failed: {}",
        String::from_utf8_lossy(&status_after.stderr)
    );
    let after_payload: Value =
        serde_json::from_slice(&status_after.stdout).expect("cleanup status json");
    assert_eq!(
        after_payload.get("configured").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        after_payload.get("provider").and_then(Value::as_str),
        Some("generic")
    );
}

#[test]
fn cleanup_run_without_init_shows_next_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["cleanup", "run"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cleanup janitor is not configured"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession cleanup init --provider auto"));
}

#[test]
fn cleanup_run_dry_and_apply_handles_hidden_and_artifact_refs() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    let remote = tmp.path().join("cleanup-remote.git");
    init_git_repo(&repo);

    run_git(
        tmp.path(),
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    run_git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    run_git(&repo, &["push", "origin", "main:main"]);

    let head_sha = first_non_empty_line(&run_git(&repo, &["rev-parse", "HEAD"]).stdout);
    let ledger_ref = opensession_git_native::branch_ledger_ref("stale/branch");
    let session_body = make_hail_jsonl("s-cleanup");
    let meta_body = serde_json::json!({
        "schema_version": 2,
        "session_id": "s-cleanup",
        "git": { "commits": [head_sha.clone()] }
    })
    .to_string();
    opensession_git_native::NativeGitStorage
        .store_session_at_ref(
            &repo,
            &ledger_ref,
            "s-cleanup",
            session_body.as_bytes(),
            meta_body.as_bytes(),
            std::slice::from_ref(&head_sha),
        )
        .expect("store hidden session");
    run_git(
        &repo,
        &["push", "origin", &format!("{ledger_ref}:{ledger_ref}")],
    );

    run_git(&repo, &["checkout", "-b", "opensession/pr-77-sessions"]);
    write_file(&repo.join("artifact.txt"), "artifact branch\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "artifact branch"]);
    run_git(
        &repo,
        &[
            "push",
            "origin",
            "opensession/pr-77-sessions:opensession/pr-77-sessions",
        ],
    );
    run_git(&repo, &["checkout", "main"]);

    let init_out = run(
        tmp.path(),
        &repo,
        &[
            "cleanup",
            "init",
            "--provider",
            "generic",
            "--remote",
            "origin",
            "--hidden-ttl-days",
            "0",
            "--artifact-ttl-days",
            "0",
            "--yes",
        ],
    );
    assert!(
        init_out.status.success(),
        "cleanup init failed: {}",
        String::from_utf8_lossy(&init_out.stderr)
    );

    let dry_run = run(tmp.path(), &repo, &["cleanup", "run", "--json"]);
    assert!(
        dry_run.status.success(),
        "cleanup dry-run failed: {}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let dry_payload: Value = serde_json::from_slice(&dry_run.stdout).expect("cleanup run json");
    assert!(
        dry_payload
            .get("hidden_candidates")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false),
        "expected hidden ref candidate in dry-run"
    );
    assert!(
        dry_payload
            .get("artifact_candidates")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false),
        "expected artifact branch candidate in dry-run"
    );

    let apply_out = run(tmp.path(), &repo, &["cleanup", "run", "--apply", "--json"]);
    assert!(
        apply_out.status.success(),
        "cleanup apply failed: {}",
        String::from_utf8_lossy(&apply_out.stderr)
    );
    let apply_payload: Value = serde_json::from_slice(&apply_out.stdout).expect("cleanup run json");
    let deleted = apply_payload
        .get("deleted")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        deleted.iter().any(|item| {
            item.as_str()
                .unwrap_or_default()
                .contains("refs/opensession/branches/")
        }),
        "expected hidden ref deletion"
    );
    assert!(
        deleted.iter().any(|item| {
            item.as_str()
                .unwrap_or_default()
                .contains("refs/heads/opensession/pr-77-sessions")
        }),
        "expected artifact branch deletion"
    );
}
