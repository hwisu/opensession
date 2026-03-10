use super::*;
use opensession_api::DesktopLifecycleCleanupState;

#[test]
fn desktop_runtime_settings_update_persists_values() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-home");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let updated = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: Some("compressed".to_string()),
        summary: Some(DesktopRuntimeSummarySettingsUpdate {
            provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                id: DesktopSummaryProviderId::Disabled,
                endpoint: String::new(),
                model: String::new(),
            },
            prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                template: format!("{DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2}\n# customized"),
            },
            response: DesktopRuntimeSummaryResponseSettingsUpdate {
                style: DesktopSummaryResponseStyle::Compact,
                shape: DesktopSummaryOutputShape::Layered,
            },
            storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                trigger: DesktopSummaryTriggerMode::OnSessionSave,
                backend: DesktopSummaryStorageBackend::HiddenRef,
            },
            source_mode: DesktopSummarySourceMode::SessionOnly,
            batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                scope: DesktopSummaryBatchScope::All,
                recent_days: 45,
            },
        }),
        vector_search: None,
        change_reader: Some(DesktopRuntimeChangeReaderSettingsUpdate {
            enabled: true,
            scope: DesktopChangeReaderScope::FullContext,
            qa_enabled: true,
            max_context_chars: 18_000,
            voice: DesktopRuntimeChangeReaderVoiceSettingsUpdate {
                enabled: true,
                provider: DesktopChangeReaderVoiceProvider::Openai,
                model: "gpt-4o-mini-tts".to_string(),
                voice: "alloy".to_string(),
                api_key: Some("sk-test-key".to_string()),
            },
        }),
        lifecycle: Some(DesktopRuntimeLifecycleSettingsUpdate {
            enabled: true,
            session_ttl_days: 45,
            summary_ttl_days: 60,
            cleanup_interval_secs: 120,
        }),
    })
    .expect("update runtime settings");
    assert_eq!(updated.session_default_view, "compressed");

    let loaded = desktop_get_runtime_settings().expect("load runtime settings");
    assert_eq!(loaded.session_default_view, "compressed");
    assert_eq!(
        loaded.summary.provider.id,
        DesktopSummaryProviderId::Disabled
    );
    assert_eq!(
        loaded.summary.response.style,
        DesktopSummaryResponseStyle::Compact
    );
    assert_eq!(
        loaded.summary.storage.backend,
        DesktopSummaryStorageBackend::HiddenRef
    );
    assert_eq!(
        loaded.summary.source_mode,
        DesktopSummarySourceMode::SessionOnly
    );
    assert_eq!(
        loaded.summary.batch.execution_mode,
        DesktopSummaryBatchExecutionMode::Manual
    );
    assert_eq!(loaded.summary.batch.scope, DesktopSummaryBatchScope::All);
    assert_eq!(loaded.summary.batch.recent_days, 45);
    assert!(loaded.summary.prompt.template.contains("customized"));
    assert!(loaded.change_reader.enabled);
    assert_eq!(
        loaded.change_reader.scope,
        DesktopChangeReaderScope::FullContext
    );
    assert!(loaded.change_reader.qa_enabled);
    assert_eq!(loaded.change_reader.max_context_chars, 18_000);
    assert!(loaded.change_reader.voice.enabled);
    assert_eq!(
        loaded.change_reader.voice.provider,
        DesktopChangeReaderVoiceProvider::Openai
    );
    assert_eq!(loaded.change_reader.voice.model, "gpt-4o-mini-tts");
    assert_eq!(loaded.change_reader.voice.voice, "alloy");
    assert!(loaded.change_reader.voice.api_key_configured);
    assert!(loaded.lifecycle.enabled);
    assert_eq!(loaded.lifecycle.session_ttl_days, 45);
    assert_eq!(loaded.lifecycle.summary_ttl_days, 60);
    assert_eq!(loaded.lifecycle.cleanup_interval_secs, 120);

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_runtime_settings_migrates_summary_storage_local_db_to_hidden_ref() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-migrate-home");
    let temp_db = unique_temp_dir("opensession-desktop-runtime-migrate-db");
    let repo_root = unique_temp_dir("opensession-desktop-runtime-migrate-repo");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );
    init_test_git_repo(&repo_root);

    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
    let mut session = build_test_session("storage-migrate-local-to-hidden");
    session.context.attributes.insert(
        "cwd".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    let source_path = repo_root.join("storage-migrate-local-to-hidden.hail.jsonl");
    std::fs::write(
        &source_path,
        session.to_jsonl().expect("serialize session jsonl"),
    )
    .expect("write session source");
    db.upsert_local_session(
        &session,
        source_path
            .to_str()
            .expect("session source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session");
    db.upsert_session_semantic_summary(&opensession_local_db::SessionSemanticSummaryUpsert {
        session_id: "storage-migrate-local-to-hidden",
        summary_json: r#"{"changes":"migrated","auth_security":"none detected","layer_file_changes":[]}"#,
        generated_at: "2026-03-05T10:00:00Z",
        provider: "codex_exec",
        model: Some("gpt-5"),
        source_kind: "session_signals",
        generation_kind: "provider",
        prompt_fingerprint: Some("migrate-fingerprint"),
        source_details_json: Some(r#"{"source":"session"}"#),
        diff_tree_json: Some(r#"[]"#),
        error: None,
    })
    .expect("insert local summary");

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(summary_settings_update_with_backend(
            DesktopSummaryStorageBackend::LocalDb,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary backend to local_db");
    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(summary_settings_update_with_backend(
            DesktopSummaryStorageBackend::HiddenRef,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("switch summary backend to hidden_ref");

    let migrated = NativeGitStorage
        .load_summary_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            "storage-migrate-local-to-hidden",
        )
        .expect("load migrated hidden_ref summary")
        .expect("migrated hidden_ref summary should exist");
    assert_eq!(migrated.provider, "codex_exec");
    assert_eq!(migrated.summary["changes"], "migrated");
    assert_eq!(migrated.model.as_deref(), Some("gpt-5"));

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
    let _ = std::fs::remove_dir_all(&repo_root);
}

#[test]
fn desktop_runtime_settings_migrates_summary_storage_hidden_ref_to_local_db() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-home");
    let temp_db = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-db");
    let repo_root = unique_temp_dir("opensession-desktop-runtime-migrate-hidden-repo");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );
    init_test_git_repo(&repo_root);

    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");
    let mut session = build_test_session("storage-migrate-hidden-to-local");
    session.context.attributes.insert(
        "cwd".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    let source_path = repo_root.join("storage-migrate-hidden-to-local.hail.jsonl");
    std::fs::write(
        &source_path,
        session.to_jsonl().expect("serialize session jsonl"),
    )
    .expect("write session source");
    db.upsert_local_session(
        &session,
        source_path
            .to_str()
            .expect("session source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session");

    let hidden_record = SessionSummaryLedgerRecord {
        session_id: "storage-migrate-hidden-to-local".to_string(),
        generated_at: "2026-03-05T11:00:00Z".to_string(),
        provider: "codex_exec".to_string(),
        model: Some("gpt-5".to_string()),
        source_kind: "session_signals".to_string(),
        generation_kind: "provider".to_string(),
        prompt_fingerprint: Some("hidden-fingerprint".to_string()),
        summary: json!({
            "changes": "from-hidden",
            "auth_security": "none detected",
            "layer_file_changes": []
        }),
        source_details: json!({ "source": "hidden_ref" }),
        diff_tree: Vec::new(),
        error: None,
    };
    NativeGitStorage
        .store_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &hidden_record)
        .expect("store hidden_ref summary");

    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(summary_settings_update_with_backend(
            DesktopSummaryStorageBackend::HiddenRef,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("set summary backend to hidden_ref");
    desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(summary_settings_update_with_backend(
            DesktopSummaryStorageBackend::LocalDb,
        )),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    })
    .expect("switch summary backend to local_db");

    let migrated = db
        .get_session_semantic_summary("storage-migrate-hidden-to-local")
        .expect("read migrated local summary")
        .expect("migrated local summary should exist");
    assert_eq!(migrated.provider, "codex_exec");
    let migrated_summary: serde_json::Value =
        serde_json::from_str(&migrated.summary_json).expect("parse migrated summary");
    assert_eq!(migrated_summary["changes"], "from-hidden");
    assert_eq!(migrated.model.as_deref(), Some("gpt-5"));

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
    let _ = std::fs::remove_dir_all(&repo_root);
}

#[test]
fn desktop_lifecycle_cleanup_deletes_expired_sessions_without_daemon() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_root = unique_temp_dir("opensession-desktop-lifecycle-cleanup");
    let repo_root = temp_root.join("repo");
    std::fs::create_dir_all(&repo_root).expect("create repo root");
    init_test_git_repo(&repo_root);

    let db = LocalDb::open_path(&temp_root.join("local.db")).expect("open local db");

    let mut expired = build_test_session("expired-session");
    expired.context.created_at = chrono::Utc::now() - chrono::Duration::days(45);
    expired.context.updated_at = expired.context.created_at;
    expired.context.attributes.insert(
        "cwd".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    let expired_source = repo_root.join("expired-session.hail.jsonl");
    std::fs::write(
        &expired_source,
        expired.to_jsonl().expect("serialize expired session"),
    )
    .expect("write expired session source");
    db.upsert_local_session(
        &expired,
        expired_source
            .to_str()
            .expect("expired session source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert expired session");

    let mut recent = build_test_session("recent-session");
    recent.context.created_at = chrono::Utc::now() - chrono::Duration::days(5);
    recent.context.updated_at = recent.context.created_at;
    recent.context.attributes.insert(
        "cwd".to_string(),
        json!(repo_root.to_string_lossy().to_string()),
    );
    let recent_source = repo_root.join("recent-session.hail.jsonl");
    std::fs::write(
        &recent_source,
        recent.to_jsonl().expect("serialize recent session"),
    )
    .expect("write recent session source");
    db.upsert_local_session(
        &recent,
        recent_source
            .to_str()
            .expect("recent session source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert recent session");

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
        .expect("store expired hidden_ref summary");
    storage
        .store_summary_at_ref(
            &repo_root,
            SUMMARY_LEDGER_REF,
            &SessionSummaryLedgerRecord {
                session_id: "recent-session".to_string(),
                generated_at: "2026-01-01T00:00:00Z".to_string(),
                provider: "codex_exec".to_string(),
                model: None,
                source_kind: "session_signals".to_string(),
                generation_kind: "provider".to_string(),
                prompt_fingerprint: None,
                summary: json!({ "changes": "recent" }),
                source_details: json!({}),
                diff_tree: vec![],
                error: None,
            },
        )
        .expect("store recent hidden_ref summary");

    let mut config = DaemonConfig::default();
    config.lifecycle.enabled = true;
    config.lifecycle.session_ttl_days = 30;
    config.lifecycle.summary_ttl_days = 30;
    config.lifecycle.cleanup_interval_secs = 60;

    super::super::run_desktop_lifecycle_cleanup_once_with_db(&config, &db)
        .expect("run desktop lifecycle cleanup");

    let lifecycle_status = super::super::desktop_lifecycle_cleanup_status_from_db(&db)
        .expect("read lifecycle cleanup status");
    assert_eq!(
        lifecycle_status.state,
        DesktopLifecycleCleanupState::Complete
    );
    assert_eq!(lifecycle_status.deleted_sessions, 1);
    assert_eq!(lifecycle_status.deleted_summaries, 1);
    assert!(
        lifecycle_status
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("Deleted 1 sessions"),
        "cleanup status should summarize deleted rows"
    );
    assert!(lifecycle_status.started_at.is_some());
    assert!(lifecycle_status.finished_at.is_some());

    assert!(
        db.get_session_by_id("expired-session")
            .expect("query expired session")
            .is_none(),
        "expired session should be deleted by desktop lifecycle cleanup"
    );
    assert!(
        db.get_session_by_id("recent-session")
            .expect("query recent session")
            .is_some(),
        "recent session should remain after desktop lifecycle cleanup"
    );
    assert!(
        storage
            .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "expired-session")
            .expect("load expired hidden_ref summary")
            .is_none(),
        "expired hidden_ref summary should be deleted by desktop lifecycle cleanup"
    );
    assert!(
        storage
            .load_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, "recent-session")
            .expect("load recent hidden_ref summary")
            .is_some(),
        "recent hidden_ref summary should remain after desktop lifecycle cleanup"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn desktop_runtime_settings_rejects_non_session_only_source_mode() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-source-lock");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(DesktopRuntimeSummarySettingsUpdate {
            provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                id: DesktopSummaryProviderId::Disabled,
                endpoint: String::new(),
                model: String::new(),
            },
            prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
            },
            response: DesktopRuntimeSummaryResponseSettingsUpdate {
                style: DesktopSummaryResponseStyle::Standard,
                shape: DesktopSummaryOutputShape::Layered,
            },
            storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                trigger: DesktopSummaryTriggerMode::OnSessionSave,
                backend: DesktopSummaryStorageBackend::HiddenRef,
            },
            source_mode: DesktopSummarySourceMode::SessionOrGitChanges,
            batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                execution_mode: DesktopSummaryBatchExecutionMode::Manual,
                scope: DesktopSummaryBatchScope::RecentDays,
                recent_days: 30,
            },
        }),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    });

    let error = result.expect_err("source mode lock should reject update");
    assert_eq!(error.status, 422);
    assert_eq!(error.code, "desktop.runtime_settings_source_mode_locked");

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_runtime_settings_rejects_zero_summary_batch_recent_days() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-summary-batch-invalid");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: Some(DesktopRuntimeSummarySettingsUpdate {
            provider: DesktopRuntimeSummaryProviderSettingsUpdate {
                id: DesktopSummaryProviderId::Disabled,
                endpoint: String::new(),
                model: String::new(),
            },
            prompt: DesktopRuntimeSummaryPromptSettingsUpdate {
                template: DEFAULT_SUMMARY_PROMPT_TEMPLATE_V2.to_string(),
            },
            response: DesktopRuntimeSummaryResponseSettingsUpdate {
                style: DesktopSummaryResponseStyle::Standard,
                shape: DesktopSummaryOutputShape::Layered,
            },
            storage: DesktopRuntimeSummaryStorageSettingsUpdate {
                trigger: DesktopSummaryTriggerMode::OnSessionSave,
                backend: DesktopSummaryStorageBackend::HiddenRef,
            },
            source_mode: DesktopSummarySourceMode::SessionOnly,
            batch: DesktopRuntimeSummaryBatchSettingsUpdate {
                execution_mode: DesktopSummaryBatchExecutionMode::OnAppStart,
                scope: DesktopSummaryBatchScope::RecentDays,
                recent_days: 0,
            },
        }),
        vector_search: None,
        change_reader: None,
        lifecycle: None,
    });

    let error = result.expect_err("recent_days=0 should fail");
    assert_eq!(error.status, 422);
    assert_eq!(
        error.code,
        "desktop.runtime_settings_invalid_summary_batch_recent_days"
    );

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[test]
fn desktop_runtime_settings_rejects_short_lifecycle_interval() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-runtime-lifecycle-invalid");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());

    let result = desktop_update_runtime_settings(DesktopRuntimeSettingsUpdateRequest {
        session_default_view: None,
        summary: None,
        vector_search: None,
        change_reader: None,
        lifecycle: Some(DesktopRuntimeLifecycleSettingsUpdate {
            enabled: true,
            session_ttl_days: 30,
            summary_ttl_days: 30,
            cleanup_interval_secs: 59,
        }),
    });

    let error = result.expect_err("cleanup interval under 60 should fail");
    assert_eq!(error.status, 422);
    assert_eq!(
        error.code,
        "desktop.runtime_settings_invalid_cleanup_interval"
    );

    let _ = std::fs::remove_dir_all(&temp_home);
}
