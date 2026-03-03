#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opensession_api::{
    DesktopApiError, DesktopContractVersionResponse, DesktopHandoffBuildRequest,
    DesktopHandoffBuildResponse, DesktopSessionListQuery, SessionRepoListResponse,
    DESKTOP_IPC_CONTRACT_VERSION,
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
    CapabilitiesResponse, LinkType, SessionDetail, SessionLink, SessionListResponse,
    SessionSummary,
};
use opensession_core::trace::Session as HailSession;
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionLink, LocalSessionRow};
use opensession_parsers::ingest::preview_parse_bytes;
use serde_json::json;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type DesktopApiResult<T> = Result<T, DesktopApiError>;

fn desktop_error(
    code: &str,
    status: u16,
    message: impl Into<String>,
    details: Option<serde_json::Value>,
) -> DesktopApiError {
    DesktopApiError {
        code: code.to_string(),
        status,
        message: message.into(),
        details,
    }
}

fn normalize_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .and_then(|trimmed| (!trimmed.is_empty()).then_some(trimmed))
}

fn parse_positive_u32(raw: Option<String>, fallback: u32, max: u32) -> u32 {
    let parsed = raw
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback);
    parsed.min(max).max(1)
}

fn map_sort_order(sort: Option<&str>) -> opensession_local_db::LocalSortOrder {
    match sort.unwrap_or_default() {
        "popular" => opensession_local_db::LocalSortOrder::Popular,
        "longest" => opensession_local_db::LocalSortOrder::Longest,
        _ => opensession_local_db::LocalSortOrder::Recent,
    }
}

fn map_time_range(time_range: Option<&str>) -> opensession_local_db::LocalTimeRange {
    match time_range.unwrap_or_default() {
        "24h" => opensession_local_db::LocalTimeRange::Hours24,
        "7d" => opensession_local_db::LocalTimeRange::Days7,
        "30d" => opensession_local_db::LocalTimeRange::Days30,
        _ => opensession_local_db::LocalTimeRange::All,
    }
}

fn build_local_filter(query: DesktopSessionListQuery) -> (LocalSessionFilter, u32, u32) {
    let page = parse_positive_u32(query.page, 1, 10_000);
    let per_page = parse_positive_u32(query.per_page, 20, 200);
    let offset = (page.saturating_sub(1)).saturating_mul(per_page);

    let filter = LocalSessionFilter {
        search: normalize_non_empty(query.search),
        tool: normalize_non_empty(query.tool),
        git_repo_name: normalize_non_empty(query.git_repo_name),
        sort: map_sort_order(query.sort.as_deref()),
        time_range: map_time_range(query.time_range.as_deref()),
        limit: Some(per_page),
        offset: Some(offset),
        ..Default::default()
    };
    (filter, page, per_page)
}

fn session_summary_from_local_row(row: LocalSessionRow) -> SessionSummary {
    SessionSummary {
        id: row.id,
        user_id: row.user_id,
        nickname: row.nickname,
        tool: row.tool,
        agent_provider: row.agent_provider,
        agent_model: row.agent_model,
        title: row.title,
        description: row.description,
        tags: row.tags,
        created_at: row.created_at.clone(),
        uploaded_at: row.uploaded_at.unwrap_or(row.created_at),
        message_count: row.message_count,
        task_count: row.task_count,
        event_count: row.event_count,
        duration_seconds: row.duration_seconds,
        total_input_tokens: row.total_input_tokens,
        total_output_tokens: row.total_output_tokens,
        git_remote: row.git_remote,
        git_branch: row.git_branch,
        git_commit: row.git_commit,
        git_repo_name: row.git_repo_name,
        pr_number: row.pr_number,
        pr_url: row.pr_url,
        working_directory: row.working_directory,
        files_modified: row.files_modified,
        files_read: row.files_read,
        has_errors: row.has_errors,
        max_active_agents: row.max_active_agents,
        session_score: 0,
        score_plugin: opensession_core::scoring::DEFAULT_SCORE_PLUGIN.to_string(),
    }
}

fn map_link_type(raw: &str) -> LinkType {
    match raw {
        "related" => LinkType::Related,
        "parent" => LinkType::Parent,
        "child" => LinkType::Child,
        _ => LinkType::Handoff,
    }
}

fn session_link_from_local(link: LocalSessionLink) -> SessionLink {
    SessionLink {
        session_id: link.session_id,
        linked_session_id: link.linked_session_id,
        link_type: map_link_type(&link.link_type),
        created_at: link.created_at,
    }
}

fn open_local_db() -> DesktopApiResult<LocalDb> {
    let custom_path = std::env::var("OPENSESSION_LOCAL_DB_PATH")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .map(PathBuf::from);

    let db = if let Some(path) = custom_path {
        LocalDb::open_path(&path)
    } else {
        LocalDb::open()
    };

    db.map_err(|error| {
        desktop_error(
            "desktop.local_db_open_failed",
            500,
            "failed to open local database",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn session_to_hail_jsonl(session: HailSession) -> DesktopApiResult<String> {
    session.to_jsonl().map_err(|error| {
        desktop_error(
            "desktop.hail_encode_failed",
            500,
            "failed to encode normalized HAIL JSONL",
            Some(json!({ "cause": error.to_string() })),
        )
    })
}

fn normalize_session_body_to_hail_jsonl(
    body: &str,
    source_path: Option<&str>,
) -> DesktopApiResult<String> {
    if let Ok(session) = HailSession::from_jsonl(body) {
        return session_to_hail_jsonl(session);
    }

    if let Ok(session) = serde_json::from_str::<HailSession>(body) {
        return session_to_hail_jsonl(session);
    }

    let filename = source_path
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("session.jsonl");

    let preview = preview_parse_bytes(filename, body.as_bytes(), None).map_err(|error| {
        desktop_error(
            "desktop.session_parse_failed",
            422,
            "failed to parse source session into HAIL format",
            Some(json!({ "cause": error.to_string(), "filename": filename })),
        )
    })?;

    session_to_hail_jsonl(preview.session)
}

fn read_and_normalize_source_session(source_path: &str) -> DesktopApiResult<String> {
    let body = std::fs::read_to_string(source_path).map_err(|error| {
        desktop_error(
            "desktop.session_source_unavailable",
            404,
            format!("session source file is unavailable ({source_path})"),
            Some(json!({ "cause": error.to_string(), "source_path": source_path })),
        )
    })?;
    normalize_session_body_to_hail_jsonl(&body, Some(source_path))
}

fn sanitize_session_id_for_filename(session_id: &str) -> String {
    let mut out = String::with_capacity(session_id.len());
    for ch in session_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "session".to_string()
    } else {
        out
    }
}

fn parse_handoff_artifact_uri(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("os://artifact/"))
        .map(ToString::to_string)
}

fn workspace_root_from_current_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml).ok()?;
            if content.contains("[workspace]") {
                return Some(dir);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn run_handoff_cli_output(input_path: &Path, pin_latest: bool) -> DesktopApiResult<Output> {
    let mut cli_args = vec![
        "handoff".to_string(),
        "build".to_string(),
        input_path.display().to_string(),
    ];
    if pin_latest {
        cli_args.push("--pin".to_string());
        cli_args.push("latest".to_string());
    }

    let env_cli_path = std::env::var("OPENSESSION_CLI_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let primary_program = env_cli_path.unwrap_or_else(|| "opensession".to_string());

    let primary_output = Command::new(&primary_program).args(&cli_args).output();
    match primary_output {
        Ok(output) => return Ok(output),
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(desktop_error(
                "desktop.handoff_cli_spawn_failed",
                500,
                "failed to execute handoff command",
                Some(json!({
                    "cause": error.to_string(),
                    "program": primary_program,
                })),
            ));
        }
        Err(_) => {}
    }

    let Some(workspace_root) = workspace_root_from_current_dir() else {
        return Err(desktop_error(
            "desktop.handoff_cli_missing",
            500,
            "handoff command is unavailable (opensession CLI not found)",
            Some(json!({
                "program": primary_program,
                "hint": "Set OPENSESSION_CLI_PATH or add opensession to PATH",
            })),
        ));
    };

    let mut cargo_args = vec![
        "run".to_string(),
        "-q".to_string(),
        "-p".to_string(),
        "opensession".to_string(),
        "--".to_string(),
    ];
    cargo_args.extend(cli_args);

    Command::new("cargo")
        .current_dir(workspace_root)
        .args(cargo_args)
        .output()
        .map_err(|error| {
            desktop_error(
                "desktop.handoff_cli_spawn_failed",
                500,
                "failed to execute cargo fallback for handoff command",
                Some(json!({ "cause": error.to_string() })),
            )
        })
}

fn write_temp_handoff_input(session_id: &str, body: &str) -> DesktopApiResult<PathBuf> {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let filename = format!(
        "opensession-handoff-{}-{}-{}.hail.jsonl",
        sanitize_session_id_for_filename(session_id),
        std::process::id(),
        now_nanos
    );
    let path = std::env::temp_dir().join(filename);
    std::fs::write(&path, body).map_err(|error| {
        desktop_error(
            "desktop.handoff_temp_write_failed",
            500,
            "failed to create temporary handoff input",
            Some(json!({ "cause": error.to_string(), "path": path })),
        )
    })?;
    Ok(path)
}

#[tauri::command]
fn desktop_get_capabilities() -> CapabilitiesResponse {
    CapabilitiesResponse::for_runtime(false, false)
}

#[tauri::command]
fn desktop_get_auth_providers() -> AuthProvidersResponse {
    AuthProvidersResponse {
        email_password: false,
        oauth: Vec::<OAuthProviderInfo>::new(),
    }
}

#[tauri::command]
fn desktop_get_contract_version() -> DesktopContractVersionResponse {
    DesktopContractVersionResponse {
        version: DESKTOP_IPC_CONTRACT_VERSION.to_string(),
    }
}

#[tauri::command]
fn desktop_list_sessions(
    query: Option<DesktopSessionListQuery>,
) -> DesktopApiResult<SessionListResponse> {
    let db = open_local_db()?;
    let (filter, page, per_page) = build_local_filter(query.unwrap_or_default());

    let total = db.count_sessions_filtered(&filter).map_err(|error| {
        desktop_error(
            "desktop.session_count_failed",
            500,
            "failed to count local sessions",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let sessions = db.list_sessions(&filter).map_err(|error| {
        desktop_error(
            "desktop.session_list_failed",
            500,
            "failed to list local sessions",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let mapped = sessions
        .into_iter()
        .map(session_summary_from_local_row)
        .collect::<Vec<_>>();

    Ok(SessionListResponse {
        sessions: mapped,
        total,
        page,
        per_page,
    })
}

#[tauri::command]
fn desktop_list_repos() -> DesktopApiResult<SessionRepoListResponse> {
    let db = open_local_db()?;
    let repos = db.list_repos().map_err(|error| {
        desktop_error(
            "desktop.repo_list_failed",
            500,
            "failed to list repository names",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    Ok(SessionRepoListResponse { repos })
}

#[tauri::command]
fn desktop_get_session_detail(id: String) -> DesktopApiResult<SessionDetail> {
    let db = open_local_db()?;
    let row = db
        .get_session_by_id(&id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_detail_failed",
                500,
                "failed to load session detail",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?
        .ok_or_else(|| {
            desktop_error(
                "desktop.session_not_found",
                404,
                "session not found",
                Some(json!({ "session_id": id })),
            )
        })?;

    let links = db
        .list_session_links(&id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_links_failed",
                500,
                "failed to load session links",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?
        .into_iter()
        .map(session_link_from_local)
        .collect::<Vec<_>>();

    Ok(SessionDetail {
        summary: session_summary_from_local_row(row),
        linked_sessions: links,
    })
}

#[tauri::command]
fn desktop_get_session_raw(id: String) -> DesktopApiResult<String> {
    let db = open_local_db()?;
    let source_path = db
        .get_session_source_path(&id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_source_path_failed",
                500,
                "failed to resolve session source path",
                Some(json!({ "cause": error.to_string(), "session_id": id })),
            )
        })?;

    if let Some(cached) = db.get_cached_body(&id).map_err(|error| {
        desktop_error(
            "desktop.session_cache_read_failed",
            500,
            "failed to read cached session body",
            Some(json!({ "cause": error.to_string(), "session_id": id })),
        )
    })? {
        match String::from_utf8(cached) {
            Ok(text) => match normalize_session_body_to_hail_jsonl(&text, source_path.as_deref()) {
                Ok(normalized) => return Ok(normalized),
                Err(error) if source_path.is_none() => return Err(error),
                Err(_) => {}
            },
            Err(error) if source_path.is_none() => {
                return Err(desktop_error(
                    "desktop.session_cache_invalid_utf8",
                    500,
                    "session body is not valid UTF-8",
                    Some(json!({ "cause": error.to_string(), "session_id": id })),
                ));
            }
            Err(_) => {}
        }
    }

    if let Some(source_path) = source_path {
        return read_and_normalize_source_session(&source_path);
    }

    Err(desktop_error(
        "desktop.session_body_not_found",
        404,
        "session body not found in local cache",
        Some(json!({ "session_id": id })),
    ))
}

#[tauri::command]
fn desktop_build_handoff(
    request: DesktopHandoffBuildRequest,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(desktop_error(
            "desktop.handoff_invalid_request",
            400,
            "session_id is required",
            None,
        ));
    }

    let db = open_local_db()?;
    let source_path = db
        .get_session_source_path(&session_id)
        .map_err(|error| {
            desktop_error(
                "desktop.session_source_path_failed",
                500,
                "failed to resolve session source path",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?
        .filter(|path| !path.trim().is_empty());

    let mut temp_input: Option<PathBuf> = None;
    let input_path: PathBuf = if let Some(source_path) = source_path.as_ref() {
        let source = PathBuf::from(source_path);
        if source.exists() {
            source
        } else {
            let cached = db.get_cached_body(&session_id).map_err(|error| {
                desktop_error(
                    "desktop.session_cache_read_failed",
                    500,
                    "failed to read cached session body",
                    Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                )
            })?;
            if let Some(cached) = cached {
                let text = String::from_utf8(cached).map_err(|error| {
                    desktop_error(
                        "desktop.session_cache_invalid_utf8",
                        500,
                        "session body is not valid UTF-8",
                        Some(json!({ "cause": error.to_string(), "session_id": session_id })),
                    )
                })?;
                let normalized = normalize_session_body_to_hail_jsonl(&text, Some(source_path))?;
                let temp = write_temp_handoff_input(&session_id, &normalized)?;
                temp_input = Some(temp.clone());
                temp
            } else {
                return Err(desktop_error(
                    "desktop.handoff_input_unavailable",
                    404,
                    "session source file is unavailable and no cached body exists",
                    Some(json!({ "session_id": session_id, "source_path": source_path })),
                ));
            }
        }
    } else {
        let cached = db.get_cached_body(&session_id).map_err(|error| {
            desktop_error(
                "desktop.session_cache_read_failed",
                500,
                "failed to read cached session body",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?;
        let Some(cached) = cached else {
            return Err(desktop_error(
                "desktop.handoff_input_unavailable",
                404,
                "session source input is unavailable",
                Some(json!({ "session_id": session_id })),
            ));
        };
        let text = String::from_utf8(cached).map_err(|error| {
            desktop_error(
                "desktop.session_cache_invalid_utf8",
                500,
                "session body is not valid UTF-8",
                Some(json!({ "cause": error.to_string(), "session_id": session_id })),
            )
        })?;
        let normalized = normalize_session_body_to_hail_jsonl(&text, None)?;
        let temp = write_temp_handoff_input(&session_id, &normalized)?;
        temp_input = Some(temp.clone());
        temp
    };

    let output = run_handoff_cli_output(&input_path, request.pin_latest);

    if let Some(temp) = temp_input {
        let _ = std::fs::remove_file(&temp);
    }

    let output = output?;
    if !output.status.success() {
        return Err(desktop_error(
            "desktop.handoff_build_failed",
            422,
            "handoff build command failed",
            Some(json!({
                "session_id": session_id,
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            })),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(artifact_uri) = parse_handoff_artifact_uri(&stdout) else {
        return Err(desktop_error(
            "desktop.handoff_parse_failed",
            500,
            "failed to parse handoff artifact URI from command output",
            Some(json!({ "stdout": stdout })),
        ));
    };

    Ok(DesktopHandoffBuildResponse {
        artifact_uri,
        pinned_alias: if request.pin_latest {
            Some("latest".to_string())
        } else {
            None
        },
    })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            desktop_get_capabilities,
            desktop_get_auth_providers,
            desktop_get_contract_version,
            desktop_list_sessions,
            desktop_list_repos,
            desktop_get_session_detail,
            desktop_get_session_raw,
            desktop_build_handoff
        ])
        .run(tauri::generate_context!())
        .expect("failed to run opensession desktop app");
}

#[cfg(test)]
mod tests {
    use super::{
        build_local_filter, desktop_get_contract_version, map_link_type,
        normalize_session_body_to_hail_jsonl, parse_handoff_artifact_uri,
        sanitize_session_id_for_filename,
        session_summary_from_local_row, DesktopSessionListQuery,
    };
    use opensession_core::trace::Session as HailSession;

    fn sample_row() -> opensession_local_db::LocalSessionRow {
        opensession_local_db::LocalSessionRow {
            id: "s1".to_string(),
            source_path: Some("/tmp/s1.hail.jsonl".to_string()),
            sync_status: "local_only".to_string(),
            last_synced_at: None,
            user_id: None,
            nickname: None,
            team_id: Some("personal".to_string()),
            tool: "codex".to_string(),
            agent_provider: Some("openai".to_string()),
            agent_model: Some("gpt-5".to_string()),
            title: Some("Sample".to_string()),
            description: Some("sample session".to_string()),
            tags: Some("tag1,tag2".to_string()),
            created_at: "2026-03-03T00:00:00Z".to_string(),
            uploaded_at: None,
            message_count: 5,
            user_message_count: 2,
            task_count: 1,
            event_count: 20,
            duration_seconds: 120,
            total_input_tokens: 100,
            total_output_tokens: 40,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            is_auxiliary: false,
        }
    }

    #[test]
    fn list_filter_defaults_page_and_per_page() {
        let (filter, page, per_page) = build_local_filter(DesktopSessionListQuery::default());
        assert_eq!(page, 1);
        assert_eq!(per_page, 20);
        assert_eq!(filter.limit, Some(20));
        assert_eq!(filter.offset, Some(0));
    }

    #[test]
    fn list_filter_parses_sort_and_range_values() {
        let (filter, page, per_page) = build_local_filter(DesktopSessionListQuery {
            page: Some("2".to_string()),
            per_page: Some("30".to_string()),
            search: Some("fix".to_string()),
            tool: Some("codex".to_string()),
            git_repo_name: Some("org/repo".to_string()),
            sort: Some("popular".to_string()),
            time_range: Some("7d".to_string()),
        });
        assert_eq!(page, 2);
        assert_eq!(per_page, 30);
        assert_eq!(filter.search.as_deref(), Some("fix"));
        assert_eq!(filter.tool.as_deref(), Some("codex"));
        assert_eq!(filter.git_repo_name.as_deref(), Some("org/repo"));
        assert_eq!(filter.offset, Some(30));
    }

    #[test]
    fn local_row_uses_created_at_when_uploaded_at_missing() {
        let summary = session_summary_from_local_row(sample_row());
        assert_eq!(summary.uploaded_at, "2026-03-03T00:00:00Z");
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
    fn desktop_contract_version_matches_shared_constant() {
        let payload = desktop_get_contract_version();
        assert_eq!(payload.version, opensession_api::DESKTOP_IPC_CONTRACT_VERSION);
    }

    #[test]
    fn parse_handoff_artifact_uri_extracts_uri_line() {
        let stdout = "building...\nos://artifact/abc123\nsaved";
        let parsed = parse_handoff_artifact_uri(stdout);
        assert_eq!(parsed.as_deref(), Some("os://artifact/abc123"));
    }

    #[test]
    fn sanitize_session_id_for_filename_replaces_path_delimiters() {
        let sanitized = sanitize_session_id_for_filename("team/repo/session:1");
        assert_eq!(sanitized, "team_repo_session_1");
    }
}
