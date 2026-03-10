use super::*;
use opensession_api::{JobManifest, job_manifest_from_session};
use opensession_core::Session;
use serde_json::json;

fn write_codex_input(path: &Path) {
    write_file(
        path,
        r#"{"type":"session_meta","timestamp":"2026-02-14T00:00:00Z","payload":{"id":"capture-job-session","timestamp":"2026-02-14T00:00:00Z","cwd":"/tmp/repo","originator":"Codex Desktop","cli_version":"0.108.0"}}
{"type":"response_item","timestamp":"2026-02-14T00:00:01Z","payload":{"type":"message","role":"user","content":"plan the job review"}}"#,
    );
}

fn inferred_job_title(job_id: &str) -> String {
    let humanized = job_id
        .split(['-', '_', '.'])
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if humanized.is_empty() {
        job_id.to_string()
    } else {
        humanized
    }
}

fn read_manifest_from_local_uri(home: &Path, repo: &Path, local_uri: &str) -> JobManifest {
    let cat_out = run(home, repo, &["cat", local_uri]);
    assert!(
        cat_out.status.success(),
        "cat failed: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );
    let session = Session::from_jsonl(&String::from_utf8_lossy(&cat_out.stdout))
        .expect("valid stored session");
    job_manifest_from_session(&session).expect("job manifest")
}

fn assert_synthesized_manifest(
    manifest: &JobManifest,
    job_id: &str,
    run_id: &str,
    stage: &str,
    review_kind: Option<&str>,
    status: &str,
    artifact_count: usize,
) {
    assert_eq!(manifest.protocol.to_string(), "opensession");
    assert_eq!(manifest.system, "opensession");
    assert_eq!(manifest.job_id, job_id);
    assert_eq!(manifest.job_title, inferred_job_title(job_id));
    assert_eq!(manifest.run_id, run_id);
    assert_eq!(manifest.attempt, 0);
    assert_eq!(manifest.stage.to_string(), stage);
    assert_eq!(
        manifest.review_kind.map(|kind| kind.to_string()),
        review_kind.map(ToOwned::to_owned)
    );
    assert_eq!(manifest.status.to_string(), status);
    assert_eq!(manifest.thread_id, None);
    assert_eq!(manifest.artifacts.len(), artifact_count);
}

#[test]
fn capture_import_registers_job_metadata_and_prints_review_url_from_partial_manifest() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    let manifest_path = repo.join("job_manifest.json");
    write_file(
        &manifest_path,
        &json!({
            "job_id": "AUTH-123",
            "run_id": "run-42",
            "review_kind": "todo",
            "artifacts": [
                {
                    "kind": "plan",
                    "label": "Plan note",
                    "uri": "file:///tmp/plan.md"
                }
            ]
        })
        .to_string(),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--profile",
            "codex",
            "--log",
            log_path.to_str().expect("log path"),
            "--manifest",
            manifest_path.to_str().expect("manifest path"),
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "capture import failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert_eq!(parsed["parser_used"], "codex");
    assert_eq!(parsed["job_id"], "AUTH-123");
    assert_eq!(parsed["run_id"], "run-42");
    assert_eq!(
        parsed["review_url"],
        "http://127.0.0.1:8788/review/job/AUTH-123?kind=todo&run_id=run-42"
    );
    let local_uri = parsed["local_uri"]
        .as_str()
        .expect("local_uri should be present")
        .to_string();
    assert!(local_uri.starts_with("os://src/local/"));
    let manifest = read_manifest_from_local_uri(tmp.path(), &repo, &local_uri);
    assert_synthesized_manifest(
        &manifest,
        "AUTH-123",
        "run-42",
        "review",
        Some("todo"),
        "pending",
        1,
    );
}

#[test]
fn capture_import_auto_detects_parser_and_sidecar_manifest() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    write_file(
        &repo.join("job_manifest.json"),
        &json!({
            "job_id": "AUTH-126",
            "review_kind": "todo"
        })
        .to_string(),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--log",
            log_path.to_str().expect("log path"),
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "capture import failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert_eq!(parsed["parser_used"], "codex");
    assert_eq!(parsed["job_id"], "AUTH-126");
    assert_eq!(parsed["run_id"], "capture-job-session");
    assert_eq!(
        parsed["review_url"],
        "http://127.0.0.1:8788/review/job/AUTH-126?kind=todo&run_id=capture-job-session"
    );

    let local_uri = parsed["local_uri"].as_str().expect("local uri");
    let manifest = read_manifest_from_local_uri(tmp.path(), &repo, local_uri);
    assert_synthesized_manifest(
        &manifest,
        "AUTH-126",
        "capture-job-session",
        "review",
        Some("todo"),
        "pending",
        0,
    );
}

#[test]
fn capture_import_uses_minimal_env_overrides_when_manifest_missing() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);

    let out = run_with_env(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--log",
            log_path.to_str().expect("log path"),
            "--json",
        ],
        &[
            ("OPENSESSION_CAPTURE_JOB_ID", "AUTH-127"),
            ("OPENSESSION_CAPTURE_RUN_ID", "run-env-42"),
            ("OPENSESSION_CAPTURE_STAGE", "review"),
            ("OPENSESSION_CAPTURE_REVIEW_KIND", "done"),
            (
                "OPENSESSION_CAPTURE_ARTIFACTS_JSON",
                r#"[{"kind":"handoff","label":"handoff","uri":"os://artifact/handoff/env"}]"#,
            ),
        ],
    );
    assert!(
        out.status.success(),
        "capture import failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert_eq!(parsed["job_id"], "AUTH-127");
    assert_eq!(parsed["run_id"], "run-env-42");
    assert_eq!(
        parsed["review_url"],
        "http://127.0.0.1:8788/review/job/AUTH-127?kind=done&run_id=run-env-42"
    );

    let local_uri = parsed["local_uri"].as_str().expect("local uri");
    let manifest = read_manifest_from_local_uri(tmp.path(), &repo, local_uri);
    assert_synthesized_manifest(
        &manifest,
        "AUTH-127",
        "run-env-42",
        "review",
        Some("done"),
        "completed",
        1,
    );
}

#[test]
fn capture_import_no_register_writes_canonical_hail_only() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    let manifest_path = repo.join("job_manifest.json");
    write_file(
        &manifest_path,
        &json!({
            "job_id": "AUTH-124",
            "run_id": "run-99",
            "stage": "planning"
        })
        .to_string(),
    );
    let out_path = repo.join("session.hail.jsonl");

    let out = run(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--profile",
            "codex",
            "--log",
            log_path.to_str().expect("log path"),
            "--manifest",
            manifest_path.to_str().expect("manifest path"),
            "--out",
            out_path.to_str().expect("out path"),
            "--no-register",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "capture import failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert_eq!(parsed["parser_used"], "codex");
    assert_eq!(parsed["job_id"], "AUTH-124");
    assert_eq!(parsed["run_id"], "run-99");
    assert!(parsed.get("local_uri").is_none() || parsed["local_uri"].is_null());
    assert_eq!(parsed["hail_path"], out_path.display().to_string());
    assert!(parsed.get("review_url").is_none() || parsed["review_url"].is_null());

    let hail_raw = fs::read_to_string(&out_path).expect("canonical hail");
    let session = Session::from_jsonl(&hail_raw).expect("valid HAIL");
    let manifest = job_manifest_from_session(&session).expect("job manifest in session");
    assert_synthesized_manifest(
        &manifest, "AUTH-124", "run-99", "planning", None, "pending", 0,
    );
    let local_db_path = tmp
        .path()
        .join(".local")
        .join("share")
        .join("opensession")
        .join("local.db");
    assert!(
        !local_db_path.exists(),
        "no-register should not create a local db"
    );
}

#[test]
fn capture_import_rejects_invalid_manifest_with_guidance() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    let manifest_path = repo.join("job_manifest.json");
    write_file(
        &manifest_path,
        &json!({
            "job_id": "AUTH-125",
            "run_id": "run-bad",
            "stage": "review"
        })
        .to_string(),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--profile",
            "codex",
            "--log",
            log_path.to_str().expect("log path"),
            "--manifest",
            manifest_path.to_str().expect("manifest path"),
        ],
    );
    assert!(
        !out.status.success(),
        "capture import unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("manifest.review_kind is required when stage=review"));
    assert!(stderr.contains("next:"));
    assert!(stderr.contains("fix manifest/env values and retry"));
}

#[test]
fn capture_import_accepts_explicit_handoff_stage_from_env() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);

    let out = run_with_env(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--log",
            log_path.to_str().expect("log path"),
            "--json",
        ],
        &[
            ("OPENSESSION_CAPTURE_JOB_ID", "AUTH-128"),
            ("OPENSESSION_CAPTURE_RUN_ID", "run-handoff"),
            ("OPENSESSION_CAPTURE_STAGE", "handoff"),
            (
                "OPENSESSION_CAPTURE_ARTIFACTS_JSON",
                r#"[{"kind":"handoff","label":"handoff","uri":"os://artifact/handoff/final"}]"#,
            ),
        ],
    );
    assert!(
        out.status.success(),
        "capture import failed\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json output");
    assert_eq!(parsed["job_id"], "AUTH-128");
    assert_eq!(parsed["run_id"], "run-handoff");
    assert!(parsed.get("review_url").is_none() || parsed["review_url"].is_null());

    let local_uri = parsed["local_uri"].as_str().expect("local uri");
    let manifest = read_manifest_from_local_uri(tmp.path(), &repo, local_uri);
    assert_synthesized_manifest(
        &manifest,
        "AUTH-128",
        "run-handoff",
        "handoff",
        None,
        "completed",
        1,
    );
}

#[test]
fn capture_import_rejects_removed_manifest_keys_with_guidance() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    let manifest_path = repo.join("job_manifest.json");
    write_file(
        &manifest_path,
        &json!({
            "job_id": "AUTH-129",
            "system": "symphony"
        })
        .to_string(),
    );

    let out = run(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--log",
            log_path.to_str().expect("log path"),
            "--manifest",
            manifest_path.to_str().expect("manifest path"),
        ],
    );
    assert!(
        !out.status.success(),
        "capture import unexpectedly succeeded"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to parse manifest"));
    assert!(stderr.contains("system"));
    assert!(
        stderr.contains(
            "use only partial manifest keys: job_id, run_id, stage, review_kind, artifacts"
        )
    );
    assert!(stderr.contains("synthesized automatically"));
}

#[test]
fn capture_import_rejects_removed_env_overrides_with_guidance() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);

    let out = run_with_env(
        tmp.path(),
        &repo,
        &[
            "capture",
            "import",
            "--log",
            log_path.to_str().expect("log path"),
        ],
        &[("OPENSESSION_CAPTURE_SYSTEM", "symphony")],
    );
    assert!(
        !out.status.success(),
        "capture import unexpectedly succeeded"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("capture import no longer supports legacy env overrides"));
    assert!(stderr.contains("OPENSESSION_CAPTURE_SYSTEM"));
    assert!(stderr.contains("use only supported capture env overrides"));
    assert!(stderr.contains("OPENSESSION_CAPTURE_JOB_ID"));
    assert!(stderr.contains("synthesized automatically"));
}
