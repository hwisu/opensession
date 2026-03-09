use super::*;

#[test]
fn review_rejects_removed_tui_view_mode() {
    let tmp = make_home();
    let (reviewer_repo, pr_link) = setup_review_fixture(&tmp, true);

    let out = run(
        tmp.path(),
        &reviewer_repo,
        &["review", &pr_link, "--view", "tui", "--no-fetch"],
    );
    assert!(
        !out.status.success(),
        "review --view tui should be rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid value 'tui'"),
        "unexpected stderr: {stderr}"
    );
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
    assert!(
        first_commit["semantic_summary"].is_object(),
        "expected commit semantic_summary payload"
    );
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

    let bundle_path = payload["bundle_path"]
        .as_str()
        .expect("bundle path in payload");
    let bundle_raw = fs::read(bundle_path).expect("read review bundle");
    let bundle_json: Value = serde_json::from_slice(&bundle_raw).expect("bundle json");
    let first_commit = bundle_json["commits"]
        .as_array()
        .and_then(|rows| rows.first())
        .expect("first commit row");
    assert!(
        first_commit["semantic_summary"].is_object(),
        "expected semantic summary even when mapped sessions are absent"
    );
}

#[test]
fn review_json_ignores_auxiliary_hidden_ref_sessions() {
    let tmp = make_home();
    let (reviewer_repo, pr_link) = setup_review_fixture_with_auxiliary(&tmp, true);

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
    assert_eq!(payload["session_count"].as_u64().unwrap_or(0), 1);

    let bundle_path = payload["bundle_path"]
        .as_str()
        .expect("bundle path in payload");
    let bundle_raw = fs::read(bundle_path).expect("read review bundle");
    let bundle_json: Value = serde_json::from_slice(&bundle_raw).expect("bundle json");
    let session_ids = bundle_json["commits"][0]["session_ids"]
        .as_array()
        .expect("session ids");
    assert_eq!(session_ids.len(), 1);
    assert_eq!(session_ids[0].as_str().unwrap_or_default(), "s-review");
}
