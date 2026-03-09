use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
fn view_without_target_defaults_to_sessions_route() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["view", "--no-open", "--json"]);
    assert!(
        out.status.success(),
        "view default failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("view json");
    assert_eq!(
        payload.get("mode").and_then(Value::as_str),
        Some("sessions")
    );
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(url, "http://127.0.0.1:8788/sessions");
}

#[test]
fn view_without_target_prefills_repo_query_when_origin_matches_owner_repo() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);
    run_git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/repo.git",
        ],
    );

    let out = run(tmp.path(), &repo, &["view", "--no-open", "--json"]);
    assert!(
        out.status.success(),
        "view default failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("view json");
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(url.contains("/sessions?"), "unexpected url: {url}");
    assert!(
        url.contains("git_repo_name=acme%2Frepo"),
        "unexpected url: {url}"
    );
}

#[test]
fn view_rejects_removed_tui_flag() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(tmp.path(), &repo, &["view", "--tui"]);
    assert!(!out.status.success(), "view --tui should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unexpected argument '--tui'"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn view_without_target_open_mode_fails_closed_without_explicit_base_url() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);
    run_git(
        &repo,
        &["config", "--local", "opensession.open-target", "web"],
    );

    let out = run(tmp.path(), &repo, &["view", "--json"]);
    assert!(
        !out.status.success(),
        "view default should fail without local server/base URL"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("local sessions server is unavailable"));
    assert!(stderr.contains("opensession view --no-open"));
    assert!(stderr.contains("opensession config init --base-url"));
}

#[cfg(target_os = "macos")]
#[test]
fn view_without_target_open_mode_suppresses_desktop_open_probe_stderr() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).expect("create bin");
    let fake_open = bin.join("open");
    write_file(
        &fake_open,
        "#!/bin/sh\necho OPEN_PROBE_MARKER >&2\nexit 1\n",
    );
    let mut perms = fs::metadata(&fake_open)
        .expect("stat fake open")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_open, perms).expect("chmod fake open");

    let base_path = std::env::var("PATH").unwrap_or_default();
    let path_env = if base_path.is_empty() {
        bin.display().to_string()
    } else {
        format!("{}:{}", bin.display(), base_path)
    };

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    let out = cmd
        .args(["view", "--json"])
        .current_dir(&repo)
        .env("HOME", tmp.path())
        .env("NO_COLOR", "1")
        .env("PATH", path_env)
        .output()
        .expect("run opensession");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("OPEN_PROBE_MARKER"),
        "desktop open probe stderr leaked: {stderr}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn view_without_target_web_open_target_skips_desktop_probe() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);
    run_git(
        &repo,
        &["config", "--local", "opensession.open-target", "web"],
    );

    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin).expect("create bin");
    let marker_path = tmp.path().join("open-invoked");
    let fake_open = bin.join("open");
    write_file(
        &fake_open,
        format!(
            "#!/bin/sh\nprintf invoked > \"{}\"\nexit 1\n",
            marker_path.display()
        )
        .as_str(),
    );
    let mut perms = fs::metadata(&fake_open)
        .expect("stat fake open")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_open, perms).expect("chmod fake open");

    let base_path = std::env::var("PATH").unwrap_or_default();
    let path_env = if base_path.is_empty() {
        bin.display().to_string()
    } else {
        format!("{}:{}", bin.display(), base_path)
    };

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_opensession"));
    let out = cmd
        .args(["view", "--json"])
        .current_dir(&repo)
        .env("HOME", tmp.path())
        .env("NO_COLOR", "1")
        .env("PATH", path_env)
        .output()
        .expect("run opensession");

    assert!(!out.status.success());
    assert!(
        !marker_path.exists(),
        "desktop open probe should be skipped for open-target=web"
    );
}

#[test]
fn view_invalid_target_shows_next_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let out = run(
        tmp.path(),
        &repo,
        &["view", "definitely-not-a-real-target", "--no-open"],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unable to resolve view target"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession view os://src/... --no-open"));
}
