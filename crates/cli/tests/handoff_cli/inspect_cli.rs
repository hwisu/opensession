use super::*;

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
