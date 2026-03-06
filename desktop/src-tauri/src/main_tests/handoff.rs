use super::*;

#[test]
fn parse_cli_quick_share_response_decodes_expected_fields() {
    let stdout = r#"{
  "uri": "os://src/git/cmVtb3Rl/ref/refs%2Fheads%2Fmain/path/sessions%2Fa.jsonl",
  "source_uri": "os://src/local/abc123",
  "remote": "https://github.com/org/repo.git",
  "push_cmd": "git push origin refs/opensession/branches/bWFpbg:refs/opensession/branches/bWFpbg",
  "quick": true,
  "pushed": true,
  "auto_push_consent": true
}"#;
    let parsed = parse_cli_quick_share_response(stdout).expect("parse quick-share payload");
    assert_eq!(parsed.source_uri, "os://src/local/abc123");
    assert_eq!(
        parsed.shared_uri,
        "os://src/git/cmVtb3Rl/ref/refs%2Fheads%2Fmain/path/sessions%2Fa.jsonl"
    );
    assert_eq!(parsed.remote, "https://github.com/org/repo.git");
    assert!(parsed.pushed);
    assert!(parsed.auto_push_consent);
}

#[test]
fn desktop_quick_share_rejects_empty_session_id() {
    let err = desktop_share_session_quick(DesktopQuickShareRequest {
        session_id: "   ".to_string(),
        remote: None,
    })
    .expect_err("empty session_id should fail");
    assert_eq!(err.code, "desktop.quick_share_invalid_request");
    assert_eq!(err.status, 400);
}

#[test]
fn handoff_canonicalization_orders_by_session_id() {
    let mut session_b = HailSession::new(
        "session-b".to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session_b.recompute_stats();
    let mut session_a = HailSession::new(
        "session-a".to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session_a.recompute_stats();

    let summaries = vec![
        HandoffSummary::from_session(&session_b),
        HandoffSummary::from_session(&session_a),
    ];
    let canonical = canonicalize_summaries(&summaries).expect("canonicalize summaries");
    let first_line = canonical.lines().next().expect("canonical line");
    assert!(first_line.contains("\"source_session_id\":\"session-a\""));
}

#[test]
fn handoff_pin_alias_validation_rejects_spaces() {
    assert!(validate_pin_alias("latest").is_ok());
    assert!(validate_pin_alias("bad alias").is_err());
}

#[test]
fn artifact_path_rejects_invalid_hash() {
    assert!(artifact_path_for_hash(Path::new("/tmp"), "abc").is_err());
}

#[test]
fn handoff_build_writes_artifact_record_and_pin() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    let repo_root = std::env::temp_dir().join(format!("opensession-desktop-handoff-{unique}"));
    let git_dir = repo_root.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create repo .git");

    let mut session = HailSession::new(
        "session-handoff-test".to_string(),
        Agent {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            tool: "codex".to_string(),
            tool_version: None,
        },
    );
    session.recompute_stats();
    let normalized = session.to_jsonl().expect("serialize session");

    let response =
        build_handoff_artifact_record(&normalized, session, true, &repo_root).expect("build");
    let hash = response
        .artifact_uri
        .strip_prefix("os://artifact/")
        .expect("artifact uri prefix");
    assert_eq!(hash.len(), 64);
    assert_eq!(response.pinned_alias.as_deref(), Some("latest"));
    let expected_download_file_name = format!("handoff-{hash}.jsonl");
    assert_eq!(
        response.download_file_name.as_deref(),
        Some(expected_download_file_name.as_str())
    );
    assert!(
        response
            .download_content
            .as_deref()
            .is_some_and(|value| value.contains("\"source_session_id\":\"session-handoff-test\""))
    );

    let artifact_path = repo_root
        .join(".opensession")
        .join("artifacts")
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.json"));
    assert!(artifact_path.exists());

    let pin_path = repo_root
        .join(".opensession")
        .join("artifacts")
        .join("pins")
        .join("latest");
    let pin_hash = std::fs::read_to_string(&pin_path).expect("read pin hash");
    assert_eq!(pin_hash.trim(), hash);

    let _ = std::fs::remove_dir_all(&repo_root);
}
