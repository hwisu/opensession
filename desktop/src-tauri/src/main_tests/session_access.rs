use super::*;

#[test]
fn list_filter_defaults_page_and_per_page() {
    let (filter, page, per_page, mode) =
        build_local_filter_with_mode(DesktopSessionListQuery::default());
    assert_eq!(page, 1);
    assert_eq!(per_page, 20);
    assert_eq!(mode, SearchMode::Keyword);
    assert_eq!(filter.limit, Some(20));
    assert_eq!(filter.offset, Some(0));
    assert!(filter.exclude_low_signal);
}

#[test]
fn list_filter_parses_sort_and_range_values() {
    let (filter, page, per_page, mode) = build_local_filter_with_mode(DesktopSessionListQuery {
        page: Some("2".to_string()),
        per_page: Some("30".to_string()),
        search: Some("fix".to_string()),
        tool: Some("codex".to_string()),
        git_repo_name: Some("org/repo".to_string()),
        sort: Some("popular".to_string()),
        time_range: Some("7d".to_string()),
        force_refresh: None,
        ..DesktopSessionListQuery::default()
    });
    assert_eq!(page, 2);
    assert_eq!(per_page, 30);
    assert_eq!(mode, SearchMode::Keyword);
    assert_eq!(filter.search.as_deref(), Some("fix"));
    assert_eq!(filter.tool.as_deref(), Some("codex"));
    assert_eq!(filter.git_repo_name.as_deref(), Some("org/repo"));
    assert_eq!(filter.offset, Some(30));
}

#[test]
fn split_search_mode_detects_vector_prefix() {
    let (query, mode) = split_search_mode(Some("vector: auth regression".to_string()));
    assert_eq!(query.as_deref(), Some("auth regression"));
    assert_eq!(mode, SearchMode::Vector);

    let (query, mode) = split_search_mode(Some("fix parser".to_string()));
    assert_eq!(query.as_deref(), Some("fix parser"));
    assert_eq!(mode, SearchMode::Keyword);
}

#[test]
fn list_filter_with_mode_keeps_vector_query_text() {
    let (filter, page, per_page, mode) = build_local_filter_with_mode(DesktopSessionListQuery {
        search: Some("vec: paging bug".to_string()),
        ..DesktopSessionListQuery::default()
    });
    assert_eq!(page, 1);
    assert_eq!(per_page, 20);
    assert_eq!(mode, SearchMode::Vector);
    assert_eq!(filter.search.as_deref(), Some("paging bug"));
}

#[test]
fn desktop_list_sessions_hides_low_signal_metadata_only_rows() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_root = unique_temp_dir("opensession-desktop-low-signal-filter");
    let db_path = temp_root.join("local.db");
    let source_path = temp_root
        .join(".claude")
        .join("projects")
        .join("fixture")
        .join("metadata-only.jsonl");
    let _db_env = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", db_path.as_os_str());

    std::fs::create_dir_all(
        source_path
            .parent()
            .expect("metadata-only source parent must exist"),
    )
    .expect("create metadata-only source dir");
    std::fs::write(
        &source_path,
        r#"{"type":"file-history-snapshot","files":[]}"#,
    )
    .expect("write metadata-only source fixture");

    let session = HailSession::new(
        "metadata-only-session".to_string(),
        Agent {
            provider: "anthropic".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            tool: "claude-code".to_string(),
            tool_version: None,
        },
    );
    let db = LocalDb::open_path(&db_path).expect("open isolated local db");
    db.upsert_local_session(
        &session,
        source_path
            .to_str()
            .expect("metadata-only source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert metadata-only local session");

    let listed = desktop_list_sessions(None).expect("list sessions");
    assert!(
        !listed
            .sessions
            .iter()
            .any(|row| row.id == "metadata-only-session"),
        "metadata-only sessions must be excluded from the default desktop session list",
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn desktop_list_sessions_force_refresh_reindexes_discovered_sessions() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_home = unique_temp_dir("opensession-desktop-force-refresh-home");
    let temp_db = unique_temp_dir("opensession-desktop-force-refresh-db");
    let _home_env = EnvVarGuard::set("HOME", temp_home.as_os_str());
    let _codex_home_env = EnvVarGuard::set("CODEX_HOME", temp_home.join(".codex").as_os_str());
    let _db_env = EnvVarGuard::set(
        "OPENSESSION_LOCAL_DB_PATH",
        temp_db.join("local.db").as_os_str(),
    );

    let session_jsonl = [
        r#"{"timestamp":"2026-03-05T00:00:00.097Z","type":"session_meta","payload":{"id":"force-refresh-session","timestamp":"2026-03-05T00:00:00.075Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.94.0"}}"#,
        r#"{"timestamp":"2026-03-05T00:00:00.119Z","type":"event_msg","payload":{"type":"user_message","message":"force refresh regression fixture"}}"#,
        r#"{"timestamp":"2026-03-05T00:00:01.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#,
    ]
    .join("\n");
    let discovered_path = temp_home
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("03")
        .join("05")
        .join("rollout-force-refresh-session.jsonl");
    std::fs::create_dir_all(
        discovered_path
            .parent()
            .expect("session discovery parent must exist"),
    )
    .expect("create session discovery dir");
    std::fs::write(&discovered_path, session_jsonl).expect("write discovered session");

    let before = desktop_list_sessions(None).expect("list sessions before force refresh");
    assert!(
        !before
            .sessions
            .iter()
            .any(|row| row.id == "force-refresh-session"),
        "session should not exist in DB before force refresh reindex"
    );

    let after = desktop_list_sessions(Some(DesktopSessionListQuery {
        force_refresh: Some(true),
        ..DesktopSessionListQuery::default()
    }))
    .expect("list sessions after force refresh");
    assert!(
        after
            .sessions
            .iter()
            .any(|row| row.id == "force-refresh-session"),
        "force refresh should reindex discovered session"
    );

    let _ = std::fs::remove_dir_all(&temp_home);
    let _ = std::fs::remove_dir_all(&temp_db);
}

#[test]
fn force_refresh_discovery_tools_skip_cursor_for_fast_path() {
    let tools = force_refresh_discovery_tools();
    assert!(tools.contains(&"codex"));
    assert!(!tools.contains(&"cursor"));
}

#[test]
fn normalize_launch_route_accepts_relative_session_path() {
    assert_eq!(
        normalize_launch_route("/sessions?git_repo_name=org%2Frepo"),
        Some("/sessions?git_repo_name=org%2Frepo".to_string())
    );
}

#[test]
fn normalize_launch_route_rejects_invalid_values() {
    assert_eq!(normalize_launch_route(""), None);
    assert_eq!(
        normalize_launch_route("https://opensession.io/sessions"),
        None
    );
    assert_eq!(normalize_launch_route("//sessions"), None);
}

#[test]
fn local_row_uses_created_at_when_uploaded_at_missing() {
    let summary = session_summary_from_local_row(sample_row());
    assert_eq!(summary.uploaded_at, "2026-03-03T00:00:00Z");
    let job_context = summary.job_context.expect("job context should map through");
    assert_eq!(job_context.job_id, "AUTH-123");
    assert!(matches!(
        job_context.review_kind,
        Some(opensession_api::JobReviewKind::Todo)
    ));
    assert_eq!(
        summary.score_plugin,
        opensession_core::scoring::DEFAULT_SCORE_PLUGIN
    );
}

#[test]
fn unknown_link_type_falls_back_to_handoff() {
    let link = map_link_type("unknown-link");
    assert!(matches!(link, opensession_api::LinkType::Handoff));
}

#[test]
fn normalize_session_body_preserves_hail_jsonl() {
    let hail_jsonl = [
        r#"{"type":"header","version":"hail-1.0.0","session_id":"hail-1","agent":{"provider":"openai","model":"gpt-5","tool":"codex"},"context":{"title":"Title","description":"Desc","tags":[],"created_at":"2026-03-03T00:00:00Z","updated_at":"2026-03-03T00:00:00Z","related_session_ids":[],"attributes":{}}}"#,
        r#"{"type":"event","event_id":"e1","timestamp":"2026-03-03T00:00:00Z","event_type":{"type":"UserMessage"},"content":{"blocks":[{"type":"Text","text":"hello"}]},"attributes":{}}"#,
        r#"{"type":"stats","event_count":1,"message_count":1,"tool_call_count":0,"task_count":0,"duration_seconds":0,"total_input_tokens":0,"total_output_tokens":0,"user_message_count":1,"files_changed":0,"lines_added":0,"lines_removed":0}"#,
    ]
    .join("\n");

    let normalized = normalize_session_body_to_hail_jsonl(&hail_jsonl, None)
        .expect("hail JSONL should normalize");
    let parsed = HailSession::from_jsonl(&normalized).expect("must remain valid hail");
    assert_eq!(parsed.session_id, "hail-1");
}

#[test]
fn normalize_session_body_converts_claude_jsonl() {
    let claude_jsonl = r#"{"type":"user","uuid":"u1","sessionId":"claude-1","timestamp":"2026-03-03T00:00:00Z","message":{"role":"user","content":"hello from claude"},"cwd":"/tmp/project","gitBranch":"main"}"#;

    let normalized = normalize_session_body_to_hail_jsonl(
        claude_jsonl,
        Some("/tmp/70dafb43-dbdd-4009-beb0-b6ac2bd9c4d1.jsonl"),
    )
    .expect("claude JSONL should parse into HAIL");
    let parsed = HailSession::from_jsonl(&normalized).expect("must be valid hail");
    assert_eq!(parsed.session_id, "claude-1");
    assert_eq!(parsed.agent.tool, "claude-code");
    assert!(!parsed.events.is_empty());
}

#[test]
fn normalize_session_body_prefers_source_path_parser_over_extension_fallback() {
    let temp_root = unique_temp_dir("opensession-desktop-normalize-source-path");
    let source_path = temp_root
        .join(".claude")
        .join("projects")
        .join("fixture")
        .join("f0639ede-3aac-4f67-a979-b175ea5f9557.jsonl");
    std::fs::create_dir_all(
        source_path
            .parent()
            .expect("claude fixture parent directory must exist"),
    )
    .expect("create claude fixture directory");

    let snapshot_only = r#"{"type":"file-history-snapshot","files":[]}"#;
    std::fs::write(&source_path, snapshot_only).expect("write claude snapshot fixture");

    let normalized = normalize_session_body_to_hail_jsonl(
        snapshot_only,
        Some(
            source_path
                .to_str()
                .expect("fixture source path must be valid utf-8"),
        ),
    )
    .expect("source-path parser should normalize claude snapshot fixture");
    let parsed = HailSession::from_jsonl(&normalized).expect("must be valid hail");
    assert_eq!(parsed.agent.tool, "claude-code");
    assert_eq!(parsed.session_id, "f0639ede-3aac-4f67-a979-b175ea5f9557");

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn desktop_contract_version_matches_shared_constant() {
    let payload = desktop_get_contract_version();
    assert_eq!(
        payload.version,
        opensession_api::DESKTOP_IPC_CONTRACT_VERSION
    );
}

#[test]
fn desktop_list_detail_raw_flow_uses_isolated_db() {
    let _env_lock = TEST_ENV_LOCK.lock().expect("test env lock");
    let temp_root = unique_temp_dir("opensession-desktop-list-detail-raw");
    let db_path = temp_root.join("local.db");
    let source_path = temp_root.join("session.hail.jsonl");
    let _db_env = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", db_path.as_os_str());

    let session = build_test_session("desktop-flow-session");
    let session_jsonl = session.to_jsonl().expect("serialize session jsonl");
    std::fs::write(&source_path, &session_jsonl).expect("write session source");

    let db = LocalDb::open_path(&db_path).expect("open isolated local db");
    db.upsert_local_session(
        &session,
        source_path
            .to_str()
            .expect("session source path must be valid utf-8"),
        &GitContext::default(),
    )
    .expect("upsert local session");

    let listed = desktop_list_sessions(None).expect("list sessions");
    assert!(
        listed
            .sessions
            .iter()
            .any(|row| row.id == session.session_id),
        "session list must include inserted session",
    );

    let detail =
        desktop_get_session_detail(session.session_id.clone()).expect("get session detail");
    assert_eq!(detail.summary.id, session.session_id);

    let raw = desktop_get_session_raw(detail.summary.id.clone()).expect("get raw session");
    assert!(
        raw.contains("\"session_id\":\"desktop-flow-session\""),
        "raw session output should include the normalized session id",
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&temp_root);
}
