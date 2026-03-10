use super::config_resolution::{
    resolve_git_retention_schedule, resolve_lifecycle_schedule, resolve_publish_mode,
    should_auto_upload,
};
use super::helpers::{build_session_meta_json, session_cwd, session_to_hail_jsonl_bytes};
use super::lifecycle::{run_lifecycle_cleanup_on_start, run_lifecycle_cleanup_once};
use super::pipeline::{maybe_generate_semantic_summary, store_locally};
use crate::config::{
    DaemonConfig, DaemonSettings, GitStorageMethod, PublishMode, SessionDefaultView,
};
use crate::repo_registry::RepoRegistry;
use chrono::Utc;
use opensession_core::{Agent, Content, Event, EventType, Session};
use opensession_git_native::{NativeGitStorage, SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord};
use opensession_local_db::LocalDb;
use opensession_runtime_config::{SummaryProvider, SummaryStorageBackend, SummaryTriggerMode};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

fn make_session_with_attrs(attrs: HashMap<String, serde_json::Value>) -> Session {
    let mut session = Session::new(
        "test-session-id".into(),
        Agent {
            provider: "anthropic".into(),
            model: "claude-opus-4-6".into(),
            tool: "claude-code".into(),
            tool_version: None,
        },
    );
    session.context.attributes = attrs;
    session
}

fn make_interaction_fixture_session(session_id: &str) -> Session {
    let mut session = Session::new(
        session_id.to_string(),
        Agent {
            provider: "anthropic".into(),
            model: "claude-opus-4-6".into(),
            tool: "claude-code".into(),
            tool_version: None,
        },
    );
    session.events = vec![
        Event {
            event_id: format!("{session_id}-user"),
            timestamp: Utc::now(),
            event_type: EventType::UserMessage,
            task_id: None,
            content: Content::text("hello"),
            duration_ms: None,
            attributes: HashMap::new(),
        },
        Event {
            event_id: format!("{session_id}-tool"),
            timestamp: Utc::now(),
            event_type: EventType::ToolCall {
                name: "write_file".to_string(),
            },
            task_id: None,
            content: Content::text(""),
            duration_ms: None,
            attributes: HashMap::new(),
        },
    ];
    session.recompute_stats();
    session
}

fn init_git_repo(path: &Path) {
    let status = Command::new("git")
        .arg("init")
        .current_dir(path)
        .status()
        .expect("git init should run");
    assert!(status.success(), "git init should succeed");

    let status = Command::new("git")
        .args(["config", "user.email", "test@opensession.local"])
        .current_dir(path)
        .status()
        .expect("git config user.email should run");
    assert!(status.success(), "git config user.email should succeed");

    let status = Command::new("git")
        .args(["config", "user.name", "OpenSession Tests"])
        .current_dir(path)
        .status()
        .expect("git config user.name should run");
    assert!(status.success(), "git config user.name should succeed");
}

#[test]
fn test_session_cwd_from_cwd_key() {
    let mut attrs = HashMap::new();
    attrs.insert("cwd".into(), json!("/home/user/project"));
    let session = make_session_with_attrs(attrs);
    assert_eq!(session_cwd(&session), Some("/home/user/project"));
}

#[test]
fn test_session_cwd_from_working_directory() {
    let mut attrs = HashMap::new();
    attrs.insert("working_directory".into(), json!("/tmp/work"));
    let session = make_session_with_attrs(attrs);
    assert_eq!(session_cwd(&session), Some("/tmp/work"));
}

#[test]
fn test_session_cwd_prefers_cwd_over_working_directory() {
    let mut attrs = HashMap::new();
    attrs.insert("cwd".into(), json!("/preferred"));
    attrs.insert("working_directory".into(), json!("/fallback"));
    let session = make_session_with_attrs(attrs);
    assert_eq!(session_cwd(&session), Some("/preferred"));
}

#[test]
fn test_session_cwd_missing() {
    let session = make_session_with_attrs(HashMap::new());
    assert_eq!(session_cwd(&session), None);
}

#[test]
fn test_session_cwd_non_string_value_returns_none() {
    let mut attrs = HashMap::new();
    attrs.insert("cwd".into(), json!(42));
    let session = make_session_with_attrs(attrs);
    assert_eq!(session_cwd(&session), None);
}

#[test]
fn test_build_session_meta_json_with_title() {
    let mut session = make_session_with_attrs(HashMap::new());
    session.context.title = Some("My Session Title".into());

    let bytes = build_session_meta_json(&session, None);
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(parsed["session_id"], "test-session-id");
    assert_eq!(parsed["schema_version"], 2);
    assert_eq!(parsed["title"], "My Session Title");
    assert_eq!(parsed["tool"], "claude-code");
    assert_eq!(parsed["model"], "claude-opus-4-6");
    assert!(parsed["stats"].is_object());
}

#[test]
fn test_build_session_meta_json_no_title() {
    let session = make_session_with_attrs(HashMap::new());

    let bytes = build_session_meta_json(&session, None);
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(parsed["session_id"], "test-session-id");
    assert!(parsed["title"].is_null());
    assert_eq!(parsed["tool"], "claude-code");
    assert_eq!(parsed["model"], "claude-opus-4-6");
}

#[test]
fn test_build_session_meta_json_includes_git_block() {
    let session = make_session_with_attrs(HashMap::new());
    let git = opensession_core::session::GitMeta {
        remote: Some("git@github.com:org/repo.git".to_string()),
        repo_name: Some("org/repo".to_string()),
        branch: Some("feature/x".to_string()),
        head: Some("abcd1234".to_string()),
        commits: vec!["abcd1234".to_string()],
    };

    let bytes = build_session_meta_json(&session, Some(&git));
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["schema_version"], 2);
    assert_eq!(parsed["git"]["repo_name"], "org/repo");
    assert_eq!(parsed["git"]["head"], "abcd1234");
}

#[test]
fn test_session_to_hail_jsonl_bytes_uses_acp_semantic_jsonl_lines() {
    let mut session = make_session_with_attrs(HashMap::new());
    session.events.push(opensession_core::Event {
        event_id: "e1".into(),
        timestamp: Utc::now(),
        event_type: opensession_core::EventType::UserMessage,
        task_id: None,
        content: opensession_core::Content::text("hello"),
        duration_ms: None,
        attributes: HashMap::new(),
    });
    session.recompute_stats();

    let body = session_to_hail_jsonl_bytes(&session).expect("serialize canonical JSONL");
    let text = String::from_utf8(body).expect("jsonl must be utf-8");
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 3, "expected session.new/update/end lines");

    let start: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(start["type"], "session.new");
    let update: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(update["type"], "session.update");
    let end: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(end["type"], "session.end");
}

#[test]
fn test_resolve_publish_mode_auto_publish_true() {
    let settings = DaemonSettings {
        auto_publish: true,
        publish_on: PublishMode::SessionEnd,
        ..Default::default()
    };
    assert_eq!(resolve_publish_mode(&settings), PublishMode::SessionEnd);
}

#[test]
fn test_resolve_publish_mode_auto_publish_false_manual() {
    let settings = DaemonSettings {
        auto_publish: false,
        publish_on: PublishMode::Manual,
        ..Default::default()
    };
    assert_eq!(resolve_publish_mode(&settings), PublishMode::Manual);
}

#[test]
fn test_resolve_publish_mode_uses_publish_on_even_when_auto_publish_false() {
    let settings = DaemonSettings {
        auto_publish: false,
        publish_on: PublishMode::Realtime,
        ..Default::default()
    };
    assert_eq!(resolve_publish_mode(&settings), PublishMode::Realtime);
}

#[test]
fn test_should_auto_upload_is_false_for_manual_mode() {
    assert!(!should_auto_upload(&PublishMode::Manual));
}

#[test]
fn test_should_auto_upload_is_true_for_session_end_and_realtime() {
    assert!(should_auto_upload(&PublishMode::SessionEnd));
    assert!(should_auto_upload(&PublishMode::Realtime));
}

#[test]
fn test_resolve_git_retention_schedule_disabled_by_default() {
    let config = DaemonConfig::default();
    assert!(resolve_git_retention_schedule(&config).is_none());
}

#[test]
fn test_resolve_git_retention_schedule_enabled_native_mode() {
    let mut config = DaemonConfig::default();
    config.git_storage.method = GitStorageMethod::Native;
    config.git_storage.retention.enabled = true;
    config.git_storage.retention.keep_days = 14;
    config.git_storage.retention.interval_secs = 120;

    let (keep_days, interval) =
        resolve_git_retention_schedule(&config).expect("retention should be enabled");
    assert_eq!(keep_days, 14);
    assert_eq!(interval, Duration::from_secs(120));
}

#[test]
fn test_resolve_git_retention_schedule_enforces_min_interval() {
    let mut config = DaemonConfig::default();
    config.git_storage.method = GitStorageMethod::Native;
    config.git_storage.retention.enabled = true;
    config.git_storage.retention.interval_secs = 0;

    let (_, interval) =
        resolve_git_retention_schedule(&config).expect("retention should be enabled");
    assert_eq!(interval, Duration::from_secs(60));
}

#[test]
fn test_resolve_lifecycle_schedule_honors_enabled_and_min_interval() {
    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = false;
    assert!(resolve_lifecycle_schedule(&config).is_none());

    config.lifecycle.enabled = true;
    config.lifecycle.cleanup_interval_secs = 12;
    assert_eq!(
        resolve_lifecycle_schedule(&config),
        Some(Duration::from_secs(60))
    );

    config.lifecycle.cleanup_interval_secs = 120;
    assert_eq!(
        resolve_lifecycle_schedule(&config),
        Some(Duration::from_secs(120))
    );
}

#[test]
fn test_run_lifecycle_cleanup_on_start_runs_immediately() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let mut expired = make_interaction_fixture_session("startup-expired-session");
    expired.context.created_at = Utc::now() - chrono::Duration::days(90);
    expired.context.updated_at = expired.context.created_at;
    db.upsert_local_session(
        &expired,
        "/tmp/startup-expired-session.jsonl",
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert expired session");

    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = true;
    config.lifecycle.session_ttl_days = 30;
    config.lifecycle.summary_ttl_days = 30;
    config.lifecycle.cleanup_interval_secs = 3600;

    run_lifecycle_cleanup_on_start(&config, &db, &RepoRegistry::default());

    assert!(
        db.get_session_by_id("startup-expired-session")
            .expect("query expired session")
            .is_none(),
        "expired session should be deleted during startup lifecycle cleanup"
    );
}

#[test]
fn test_run_lifecycle_cleanup_deletes_expired_sessions_and_hidden_ref_summaries() {
    let tmp = tempdir().expect("tempdir");
    let repo_root = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("create repo root");
    init_git_repo(&repo_root);

    let db_path = tmp.path().join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");
    let mut expired = make_interaction_fixture_session("expired-session");
    expired.context.created_at = Utc::now() - chrono::Duration::days(90);
    expired.context.updated_at = expired.context.created_at;
    expired.context.attributes.insert(
        "working_directory".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    db.upsert_local_session(
        &expired,
        "/tmp/expired-session.jsonl",
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert expired session");

    let mut active = make_interaction_fixture_session("active-session");
    active.context.attributes.insert(
        "working_directory".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    db.upsert_local_session(
        &active,
        "/tmp/active-session.jsonl",
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert active session");

    let storage = NativeGitStorage;
    storage
        .store_summary_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            &SessionSummaryLedgerRecord {
                session_id: "expired-session".to_string(),
                generated_at: "2026-01-01T00:00:00Z".to_string(),
                provider: "codex_exec".to_string(),
                model: None,
                source_kind: "session_signals".to_string(),
                generation_kind: "provider".to_string(),
                prompt_fingerprint: None,
                summary: json!({ "changes": "expired" }),
                source_details: json!({}),
                diff_tree: vec![],
                error: None,
            },
        )
        .expect("store expired summary");
    storage
        .store_summary_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            &SessionSummaryLedgerRecord {
                session_id: "active-session".to_string(),
                generated_at: "2026-01-01T00:00:00Z".to_string(),
                provider: "codex_exec".to_string(),
                model: None,
                source_kind: "session_signals".to_string(),
                generation_kind: "provider".to_string(),
                prompt_fingerprint: None,
                summary: json!({ "changes": "active" }),
                source_details: json!({}),
                diff_tree: vec![],
                error: None,
            },
        )
        .expect("store active summary");

    let mut registry = RepoRegistry::default();
    registry
        .add(&repo_root)
        .expect("repo registry should accept repo root");

    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = true;
    config.lifecycle.session_ttl_days = 30;
    config.lifecycle.summary_ttl_days = 10_000;
    config.lifecycle.cleanup_interval_secs = 60;

    run_lifecycle_cleanup_once(&config, &db, &registry).expect("run lifecycle cleanup");

    assert!(
        db.get_session_by_id("expired-session")
            .expect("query expired session")
            .is_none(),
        "expired session should be deleted"
    );
    assert!(
        db.get_session_by_id("active-session")
            .expect("query active session")
            .is_some(),
        "active session should remain"
    );
    assert!(
        storage
            .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "expired-session")
            .expect("load expired summary")
            .is_none(),
        "hidden-ref summary for expired session should be deleted"
    );
    assert!(
        storage
            .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "active-session")
            .expect("load active summary")
            .is_some(),
        "active session summary should remain"
    );
}

#[test]
fn test_run_lifecycle_cleanup_prunes_local_summary_rows_by_ttl() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let session_old = make_interaction_fixture_session("summary-old");
    db.upsert_local_session(
        &session_old,
        "/tmp/summary-old.jsonl",
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert old summary session");
    let session_new = make_interaction_fixture_session("summary-new");
    db.upsert_local_session(
        &session_new,
        "/tmp/summary-new.jsonl",
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert new summary session");

    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id: "summary-old",
        summary_json: r#"{"changes":"old"}"#,
        generated_at: "2020-01-01T00:00:00Z",
        provider: "codex_exec",
        model: None,
        source_kind: "session_signals",
        generation_kind: "provider",
        prompt_fingerprint: None,
        source_details_json: None,
        diff_tree_json: None,
        error: None,
    })
    .expect("insert old summary");
    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id: "summary-new",
        summary_json: r#"{"changes":"new"}"#,
        generated_at: "2999-01-01T00:00:00Z",
        provider: "codex_exec",
        model: None,
        source_kind: "session_signals",
        generation_kind: "provider",
        prompt_fingerprint: None,
        source_details_json: None,
        diff_tree_json: None,
        error: None,
    })
    .expect("insert new summary");

    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = true;
    config.lifecycle.session_ttl_days = 10_000;
    config.lifecycle.summary_ttl_days = 30;
    config.lifecycle.cleanup_interval_secs = 60;

    run_lifecycle_cleanup_once(&config, &db, &RepoRegistry::default())
        .expect("run lifecycle cleanup");

    assert!(
        db.get_session_semantic_summary("summary-old")
            .expect("query old summary")
            .is_none(),
        "old summary should be pruned"
    );
    assert!(
        db.get_session_semantic_summary("summary-new")
            .expect("query new summary")
            .is_some(),
        "new summary should remain"
    );
}

#[test]
fn test_run_lifecycle_cleanup_deletes_sessions_with_missing_source_parent_dir() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let missing_parent_root = tmp.path().join("deleted-source-root");
    std::fs::create_dir_all(&missing_parent_root).expect("create missing parent root");
    let missing_parent_source = missing_parent_root.join("missing-parent.jsonl");

    let existing_parent_root = tmp.path().join("existing-source-root");
    std::fs::create_dir_all(&existing_parent_root).expect("create existing parent root");
    let existing_parent_source = existing_parent_root.join("missing-file.jsonl");

    let missing_parent_session = make_interaction_fixture_session("missing-parent-session");
    db.upsert_local_session(
        &missing_parent_session,
        missing_parent_source
            .to_str()
            .expect("missing parent source path should be utf-8"),
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert missing-parent session");

    let existing_parent_session = make_interaction_fixture_session("existing-parent-session");
    db.upsert_local_session(
        &existing_parent_session,
        existing_parent_source
            .to_str()
            .expect("existing parent source path should be utf-8"),
        &opensession_local_db::git::GitContext::default(),
    )
    .expect("upsert existing-parent session");

    std::fs::remove_dir_all(&missing_parent_root).expect("remove missing parent root");

    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = true;
    config.lifecycle.session_ttl_days = 10_000;
    config.lifecycle.summary_ttl_days = 10_000;
    config.lifecycle.cleanup_interval_secs = 60;

    run_lifecycle_cleanup_once(&config, &db, &RepoRegistry::default())
        .expect("run lifecycle cleanup");

    assert!(
        db.get_session_by_id("missing-parent-session")
            .expect("query missing-parent session")
            .is_none(),
        "session should be deleted when source parent directory is gone"
    );
    assert!(
        db.get_session_by_id("existing-parent-session")
            .expect("query existing-parent session")
            .is_some(),
        "session should remain when source parent directory still exists"
    );
}

#[test]
fn test_store_locally_uses_compressed_session_only_when_default_view_is_compressed() {
    let tmp = tempdir().expect("tempdir");
    let db_path = PathBuf::from(tmp.path()).join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let full_session = make_interaction_fixture_session("store-full");
    let mut full_config = DaemonConfig::default();
    full_config.daemon.session_default_view = SessionDefaultView::Full;
    store_locally(
        &full_session,
        Path::new("/tmp/store-full.jsonl"),
        &db,
        &full_config,
    )
    .expect("store full session");

    let stored_full = db
        .get_session_by_id("store-full")
        .expect("query full")
        .expect("full session exists");
    assert_eq!(stored_full.event_count, 2);

    let compressed_session = make_interaction_fixture_session("store-compressed");
    let mut compressed_config = DaemonConfig::default();
    compressed_config.daemon.session_default_view = SessionDefaultView::Compressed;
    store_locally(
        &compressed_session,
        Path::new("/tmp/store-compressed.jsonl"),
        &db,
        &compressed_config,
    )
    .expect("store compressed session");

    let stored_compressed = db
        .get_session_by_id("store-compressed")
        .expect("query compressed")
        .expect("compressed session exists");
    assert_eq!(stored_compressed.event_count, 1);
}

#[test]
fn test_store_locally_caches_source_body() {
    let tmp = tempdir().expect("tempdir");
    let db_path = PathBuf::from(tmp.path()).join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");
    let session = make_interaction_fixture_session("store-cache");
    let source_path = PathBuf::from(tmp.path()).join("store-cache.jsonl");
    let source_body = b"{\"source\":\"fixture\"}\n".to_vec();
    std::fs::write(&source_path, &source_body).expect("write source fixture");

    let mut config = DaemonConfig::default();
    config.daemon.session_default_view = SessionDefaultView::Full;
    store_locally(&session, &source_path, &db, &config).expect("store cached session");

    let cached = db
        .get_cached_body("store-cache")
        .expect("query body cache")
        .expect("cache row should exist");
    assert_eq!(cached, source_body);
}

#[tokio::test]
async fn test_auto_summary_runs_on_session_save_and_persists_row() {
    let tmp = tempdir().expect("tempdir");
    let db_path = PathBuf::from(tmp.path()).join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let session = make_interaction_fixture_session("summary-auto");
    let mut config = DaemonConfig::default();
    config.summary.provider.id = SummaryProvider::CodexExec;
    config.summary.storage.trigger = SummaryTriggerMode::OnSessionSave;
    config.summary.storage.backend = SummaryStorageBackend::LocalDb;

    maybe_generate_semantic_summary(&session, &db, &config)
        .await
        .expect("summary generation should not fail hard");

    let row = db
        .get_session_semantic_summary("summary-auto")
        .expect("query summary")
        .expect("summary row should exist");
    assert_eq!(row.provider, "codex_exec");
    assert_eq!(row.source_kind, "session_signals");
    assert!(!row.summary_json.trim().is_empty());
}

#[tokio::test]
async fn test_auto_summary_skips_when_trigger_mode_is_manual() {
    let tmp = tempdir().expect("tempdir");
    let db_path = PathBuf::from(tmp.path()).join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let session = make_interaction_fixture_session("summary-manual");
    let mut config = DaemonConfig::default();
    config.summary.provider.id = SummaryProvider::CodexExec;
    config.summary.storage.trigger = SummaryTriggerMode::Manual;
    config.summary.storage.backend = SummaryStorageBackend::LocalDb;

    maybe_generate_semantic_summary(&session, &db, &config)
        .await
        .expect("manual trigger should no-op");

    let row = db
        .get_session_semantic_summary("summary-manual")
        .expect("query summary");
    assert!(row.is_none());
}

#[tokio::test]
async fn test_auto_summary_skips_when_storage_backend_is_none() {
    let tmp = tempdir().expect("tempdir");
    let db_path = PathBuf::from(tmp.path()).join("local.db");
    let db = LocalDb::open_path(&db_path).expect("open local db");

    let session = make_interaction_fixture_session("summary-no-persist");
    let mut config = DaemonConfig::default();
    config.summary.provider.id = SummaryProvider::CodexExec;
    config.summary.storage.trigger = SummaryTriggerMode::OnSessionSave;
    config.summary.storage.backend = SummaryStorageBackend::None;

    maybe_generate_semantic_summary(&session, &db, &config)
        .await
        .expect("none persist should no-op");

    let row = db
        .get_session_semantic_summary("summary-no-persist")
        .expect("query summary");
    assert!(row.is_none());
}
