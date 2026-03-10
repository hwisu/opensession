use super::*;

fn ready_vector_preflight_fixture() -> DesktopVectorPreflightResponse {
    DesktopVectorPreflightResponse {
        provider: DesktopVectorSearchProvider::Ollama,
        endpoint: "http://127.0.0.1:11434".to_string(),
        model: "bge-m3".to_string(),
        ollama_reachable: true,
        model_installed: true,
        install_state: DesktopVectorInstallState::Ready,
        progress_pct: 100,
        message: Some("model is installed and ready".to_string()),
    }
}

#[test]
fn validate_vector_preflight_allows_rebuild_when_vector_disabled() {
    let preflight = ready_vector_preflight_fixture();
    let result = validate_vector_preflight_ready(&preflight, false, false);
    assert!(result.is_ok());
}

#[test]
fn validate_vector_preflight_requires_enabled_for_search_path() {
    let preflight = ready_vector_preflight_fixture();
    let err = validate_vector_preflight_ready(&preflight, false, true)
        .expect_err("search path should require vector enabled");
    assert_eq!(err.code, "desktop.vector_search_disabled");
    assert_eq!(err.status, 422);
}

#[test]
fn persist_vector_index_failure_snapshot_preserves_progress() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_db = unique_temp_dir("opensession-desktop-vector-failure-snapshot");
    let db = LocalDb::open_path(&temp_db.join("local.db")).expect("open local db");

    db.set_vector_index_job(&VectorIndexJobRow {
        status: "running".to_string(),
        processed_sessions: 7,
        total_sessions: 42,
        message: Some("indexing session-7".to_string()),
        started_at: Some("2026-03-06T00:00:00Z".to_string()),
        finished_at: None,
    })
    .expect("seed running vector job");

    let error = super::super::desktop_error(
        "desktop.vector_search_unavailable",
        422,
        "vector search endpoint returned HTTP 404",
        Some(json!({
            "endpoint": "http://127.0.0.1:11434/api/embeddings",
            "status": 404,
            "body": "{\"error\":\"model 'bge-m3' not found\"}",
            "batch_endpoint": "http://127.0.0.1:11434/api/embed",
            "batch_status": 400,
            "batch_body": "{\"error\":\"bad request\"}",
            "model": "bge-m3",
            "hint": "verify embedding model exists in local ollama"
        })),
    );
    super::super::persist_vector_index_failure_snapshot(&db, &error)
        .expect("persist vector failure snapshot");

    let snapshot = db
        .get_vector_index_job()
        .expect("read vector job")
        .expect("vector job should exist");
    assert_eq!(snapshot.status, "failed");
    assert_eq!(snapshot.processed_sessions, 7);
    assert_eq!(snapshot.total_sessions, 42);
    assert_eq!(
        snapshot.message.as_deref(),
        Some(
            "vector search endpoint returned HTTP 404\nReason: model 'bge-m3' not found\nHTTP: 404\nEndpoint: http://127.0.0.1:11434/api/embeddings\nBatch reason: bad request\nBatch HTTP: 400\nBatch endpoint: http://127.0.0.1:11434/api/embed\nModel: bge-m3\nAction: verify embedding model exists in local ollama"
        )
    );
    assert_eq!(snapshot.started_at.as_deref(), Some("2026-03-06T00:00:00Z"));
    assert!(snapshot.finished_at.is_some());

    let _ = std::fs::remove_dir_all(&temp_db);
}

#[test]
fn rebuild_vector_index_blocking_continues_after_embedding_failures() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_root = unique_temp_dir("opensession-desktop-vector-failure-continue");
    let db = LocalDb::open_path(&temp_root.join("local.db")).expect("open local db");

    for session_id in ["vector-failure-a", "vector-failure-b"] {
        let session = build_test_session(session_id);
        let source_path = temp_root.join(format!("{session_id}.jsonl"));
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
    }

    let mut runtime = DaemonConfig::default();
    runtime.vector_search.enabled = true;
    runtime.vector_search.endpoint = "http://127.0.0.1:1".to_string();
    runtime.vector_search.model = "bge-m3".to_string();

    super::super::rebuild_vector_index_blocking(&db, &runtime)
        .expect("skippable embedding failures should not abort rebuild");

    let snapshot = db
        .get_vector_index_job()
        .expect("read vector job")
        .expect("vector job should exist");
    assert_eq!(snapshot.status, "complete");
    assert_eq!(snapshot.processed_sessions, 2);
    assert_eq!(snapshot.total_sessions, 2);
    assert!(
        snapshot
            .message
            .as_deref()
            .is_some_and(|message| message.contains("2 failed"))
    );
    assert!(
        db.list_recent_vector_chunks_for_model("bge-m3", 10)
            .expect("list vector chunks")
            .is_empty(),
        "failed sessions should not leave partial vector chunks behind"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn cosine_similarity_handles_basic_cases() {
    let same = cosine_similarity(&[1.0, 0.0, 1.0], &[1.0, 0.0, 1.0]);
    assert!((same - 1.0).abs() < 1e-6);

    let orthogonal = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
    assert!(orthogonal.abs() < 1e-6);

    let mismatch = cosine_similarity(&[1.0, 2.0], &[1.0]);
    assert_eq!(mismatch, 0.0);
}

#[test]
fn extract_vector_lines_preserves_dot_line_tokens() {
    let mut session = build_test_session("vector-lines");
    session.events.push(Event {
        event_id: "evt-dot".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::AgentMessage,
        task_id: None,
        content: Content::text(".\nkeep-this-line"),
        duration_ms: None,
        attributes: std::collections::HashMap::new(),
    });
    let lines = extract_vector_lines(&session);
    assert!(lines.iter().any(|line| line == "."));
    assert!(lines.iter().any(|line| line.contains("keep-this-line")));
}

#[test]
fn build_vector_chunks_applies_overlap_rules() {
    let mut session = build_test_session("vector-chunks");
    session.events.push(Event {
        event_id: "evt-overlap".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::AgentMessage,
        task_id: None,
        content: Content::text("l1\nl2\nl3"),
        duration_ms: None,
        attributes: std::collections::HashMap::new(),
    });
    let mut runtime = DaemonConfig::default();
    runtime.vector_search.chunking_mode = opensession_runtime_config::VectorChunkingMode::Manual;
    runtime.vector_search.chunk_size_lines = 2;
    runtime.vector_search.chunk_overlap_lines = 1;
    let chunks = build_vector_chunks_for_session(&session, "source-hash", &runtime);
    assert!(chunks.len() >= 2);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 2);
    assert_eq!(chunks[1].start_line, 2);
}

#[test]
fn build_vector_chunks_auto_tunes_for_small_session() {
    let mut session = build_test_session("vector-chunks-auto");
    session.events.push(Event {
        event_id: "evt-auto".to_string(),
        timestamp: chrono::Utc::now(),
        event_type: EventType::AgentMessage,
        task_id: None,
        content: Content::text("a\nb\nc\nd\ne\nf"),
        duration_ms: None,
        attributes: std::collections::HashMap::new(),
    });
    let runtime = DaemonConfig::default();
    let chunks = build_vector_chunks_for_session(&session, "source-hash", &runtime);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 7);
}
