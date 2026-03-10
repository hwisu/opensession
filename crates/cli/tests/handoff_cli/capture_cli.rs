use super::*;
use opensession_api::job_manifest_from_session;
use opensession_core::Session;
use serde_json::json;

fn write_codex_input(path: &Path) {
    write_file(
        path,
        r#"{"type":"session_meta","session_id":"capture-job-session","timestamp":"2026-02-14T00:00:00Z"}
{"type":"response_item","timestamp":"2026-02-14T00:00:01Z","payload":{"type":"message","role":"user","content":"plan the job review"}}"#,
    );
}

#[test]
fn capture_import_registers_job_metadata_and_prints_review_url() {
    let tmp = make_home();
    let repo = tmp.path().join("repo");
    init_git_repo(&repo);

    let log_path = repo.join("codex-rollout.jsonl");
    write_codex_input(&log_path);
    let manifest_path = repo.join("job_manifest.json");
    write_file(
        &manifest_path,
        &json!({
            "protocol": "agent_communication_protocol",
            "system": "symphony",
            "job_id": "AUTH-123",
            "job_title": "Fix auth bug",
            "run_id": "run-42",
            "attempt": 1,
            "stage": "review",
            "review_kind": "todo",
            "status": "pending",
            "thread_id": "thread-9",
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

    let cat_out = run(tmp.path(), &repo, &["cat", &local_uri]);
    assert!(
        cat_out.status.success(),
        "cat failed: {}",
        String::from_utf8_lossy(&cat_out.stderr)
    );
    let hail_raw = String::from_utf8_lossy(&cat_out.stdout);
    let session = Session::from_jsonl(&hail_raw).expect("valid stored HAIL");
    let manifest = job_manifest_from_session(&session).expect("job manifest");
    assert_eq!(manifest.system, "symphony");
    assert_eq!(manifest.job_id, "AUTH-123");
    assert_eq!(manifest.run_id, "run-42");
    assert_eq!(manifest.stage.to_string(), "review");
    assert_eq!(
        manifest.review_kind.map(|kind| kind.to_string()),
        Some("todo".to_string())
    );
    assert_eq!(manifest.status.to_string(), "pending");
    assert_eq!(manifest.thread_id.as_deref(), Some("thread-9"));
    assert_eq!(manifest.artifacts.len(), 1);
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
            "protocol": "agent_client_protocol",
            "system": "symphony",
            "job_id": "AUTH-124",
            "job_title": "Review the plan",
            "run_id": "run-99",
            "attempt": 2,
            "stage": "execution",
            "status": "completed"
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
    assert_eq!(parsed["job_id"], "AUTH-124");
    assert_eq!(parsed["run_id"], "run-99");
    assert!(parsed.get("local_uri").is_none() || parsed["local_uri"].is_null());
    assert_eq!(parsed["hail_path"], out_path.display().to_string());
    assert!(parsed.get("review_url").is_none() || parsed["review_url"].is_null());

    let hail_raw = fs::read_to_string(&out_path).expect("canonical hail");
    let session = Session::from_jsonl(&hail_raw).expect("valid HAIL");
    let manifest = job_manifest_from_session(&session).expect("job manifest in session");
    assert_eq!(manifest.job_id, "AUTH-124");
    assert_eq!(manifest.run_id, "run-99");
    assert_eq!(manifest.stage.to_string(), "execution");
    assert_eq!(manifest.status.to_string(), "completed");
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
            "protocol": "agent_communication_protocol",
            "system": "symphony",
            "job_id": "AUTH-125",
            "job_title": "Broken review",
            "run_id": "run-bad",
            "attempt": 0,
            "stage": "review",
            "status": "pending"
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
    assert!(stderr.contains("fix manifest fields and retry"));
}
