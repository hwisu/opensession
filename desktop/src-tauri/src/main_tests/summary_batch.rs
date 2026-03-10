use super::*;

#[test]
fn desktop_summary_batch_run_and_status_complete_when_no_sessions() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-summary-batch-home");
    let temp_db = unique_temp_dir("opensession-desktop-summary-batch-db");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(manual_summary_batch_settings(
            DesktopSummaryStorageBackend::None,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary batch config");

    let started = desktop_summary_batch_run().expect("start summary batch");
    assert!(
        matches!(
            started.state,
            DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
        ),
        "initial state should be running or complete"
    );

    let final_state = wait_for_summary_batch_completion(started);
    assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
    assert_eq!(final_state.total_sessions, 0);
    assert_eq!(final_state.processed_sessions, 0);
    assert_eq!(final_state.failed_sessions, 0);

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
}

#[test]
fn desktop_summary_batch_skips_sessions_with_missing_source_files() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-summary-batch-skip-home");
    let temp_db = unique_temp_dir("opensession-desktop-summary-batch-skip-db");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(manual_summary_batch_settings(
            DesktopSummaryStorageBackend::None,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary batch config");

    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
    let session = build_test_session("missing-source-session");
    let missing_source = temp_db.join("missing-source-session.jsonl");
    db.upsert_local_session(
        &session,
        missing_source
            .to_str()
            .expect("missing source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session with missing source path");

    let started = desktop_summary_batch_run().expect("start summary batch");
    assert!(
        matches!(
            started.state,
            DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
        ),
        "initial state should be running or complete"
    );

    let final_state = wait_for_summary_batch_completion(started);
    assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
    assert_eq!(final_state.total_sessions, 1);
    assert_eq!(final_state.processed_sessions, 1);
    assert_eq!(final_state.failed_sessions, 0);
    assert!(
        final_state
            .message
            .as_deref()
            .is_some_and(|message| message.contains("skipped missing sources"))
    );

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
}

#[test]
fn desktop_summary_batch_skips_sessions_with_existing_local_db_summaries() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-summary-batch-local-summary-home");
    let temp_db = unique_temp_dir("opensession-desktop-summary-batch-local-summary-db");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(manual_summary_batch_settings(
            DesktopSummaryStorageBackend::HiddenRef,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary batch config");

    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
    let session = build_test_session("existing-local-summary-session");
    let missing_source = temp_db.join("existing-local-summary-session.jsonl");
    db.upsert_local_session(
        &session,
        missing_source
            .to_str()
            .expect("missing source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session with missing source path");
    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id: "existing-local-summary-session",
        summary_json: r#"{"changes":"cached","auth_security":"none detected","layer_file_changes":[]}"#,
        generated_at: "2026-03-06T01:00:00Z",
        provider: "codex_exec",
        model: Some("gpt-5"),
        source_kind: "session_signals",
        generation_kind: "provider",
        prompt_fingerprint: Some("local-cache"),
        source_details_json: Some(r#"{"source":"local_db"}"#),
        diff_tree_json: Some(r#"[]"#),
        error: None,
    })
    .expect("insert local summary");

    let started = desktop_summary_batch_run().expect("start summary batch");
    assert!(
        matches!(
            started.state,
            DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
        ),
        "initial state should be running or complete"
    );

    let final_state = wait_for_summary_batch_completion(started);
    assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
    assert_eq!(final_state.total_sessions, 0);
    assert_eq!(final_state.processed_sessions, 0);
    assert_eq!(final_state.failed_sessions, 0);
    assert!(
        final_state
            .message
            .as_deref()
            .is_some_and(|message| message.contains("already summarized"))
    );

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
}

#[test]
fn desktop_summary_batch_skips_sessions_with_existing_hidden_ref_summaries() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-home");
    let temp_db = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-db");
    let repo_root = unique_temp_dir("opensession-desktop-summary-batch-hidden-summary-repo");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );
    init_test_git_repo(&repo_root);

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(manual_summary_batch_settings(
            DesktopSummaryStorageBackend::LocalDb,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary batch config");

    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
    let mut session = build_test_session("existing-hidden-summary-session");
    session.context.attributes.insert(
        "cwd".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    let missing_source = repo_root.join("existing-hidden-summary-session.jsonl");
    db.upsert_local_session(
        &session,
        missing_source
            .to_str()
            .expect("missing source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session with missing source path");
    NativeGitStorage
        .store_summary_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            &SessionSummaryLedgerRecord {
                session_id: "existing-hidden-summary-session".to_string(),
                generated_at: "2026-03-06T02:00:00Z".to_string(),
                provider: "codex_exec".to_string(),
                model: Some("gpt-5".to_string()),
                source_kind: "session_signals".to_string(),
                generation_kind: "provider".to_string(),
                prompt_fingerprint: Some("hidden-cache".to_string()),
                summary: json!({
                    "changes": "cached",
                    "auth_security": "none detected",
                    "layer_file_changes": []
                }),
                source_details: json!({ "source": "hidden_ref" }),
                diff_tree: Vec::new(),
                error: None,
            },
        )
        .expect("store hidden_ref summary");

    let started = desktop_summary_batch_run().expect("start summary batch");
    assert!(
        matches!(
            started.state,
            DesktopSummaryBatchState::Running | DesktopSummaryBatchState::Complete
        ),
        "initial state should be running or complete"
    );

    let final_state = wait_for_summary_batch_completion(started);
    assert_eq!(final_state.state, DesktopSummaryBatchState::Complete);
    assert_eq!(final_state.total_sessions, 0);
    assert_eq!(final_state.processed_sessions, 0);
    assert_eq!(final_state.failed_sessions, 0);
    assert!(
        final_state
            .message
            .as_deref()
            .is_some_and(|message| message.contains("already summarized"))
    );

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
    let _ = std::fs::remove_dir_all(&repo_root);
}
