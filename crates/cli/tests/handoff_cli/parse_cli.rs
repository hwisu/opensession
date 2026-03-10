use super::*;

#[test]
fn register_rejects_non_hail_with_next_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let input = repo.join("not-hail.txt");
    write_file(&input, "this is not hail jsonl\n");

    let out = run(
        tmp.path(),
        &repo,
        &["register", input.to_str().expect("path")],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("register expects canonical session JSONL"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession parse --profile codex"));
}

#[test]
fn parse_invalid_profile_shows_next_steps() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let raw = repo.join("raw-session.jsonl");
    write_file(&raw, "{\"not\":\"a valid session format\"}\n");

    let out = run(
        tmp.path(),
        &repo,
        &[
            "parse",
            "--profile",
            "unknown-profile",
            raw.to_str().expect("path"),
        ],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("opensession parse --help"));
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
