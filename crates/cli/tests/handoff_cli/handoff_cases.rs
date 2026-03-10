use super::*;

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
