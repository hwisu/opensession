#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opensession_api::{
    oauth::{AuthProvidersResponse, OAuthProviderInfo},
    CapabilitiesResponse, DesktopApiError, DesktopContractVersionResponse,
    DesktopHandoffBuildRequest, DesktopHandoffBuildResponse, DesktopSessionListQuery, LinkType,
    SessionDetail, SessionLink, SessionListResponse, SessionRepoListResponse, SessionSummary,
    DESKTOP_IPC_CONTRACT_VERSION,
};
use opensession_core::handoff::{validate_handoff_summaries, HandoffSummary};
use opensession_core::object_store::{
    find_repo_root, global_store_root, sha256_hex, store_local_object,
};
use opensession_core::source_uri::SourceUri;
use opensession_core::trace::Session as HailSession;
use opensession_local_db::{LocalDb, LocalSessionFilter, LocalSessionLink, LocalSessionRow};
use opensession_parsers::ingest::preview_parse_bytes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

type DesktopApiResult<T> = Result<T, DesktopApiError>;

const HANDOFF_RECORD_VERSION: &str = "v1";
const HANDOFF_LATEST_PIN_ALIAS: &str = "latest";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopHandoffArtifactRecord {
    version: String,
    sha256: String,
    created_at: String,
    source_uris: Vec<String>,
    canonical_jsonl: String,
    raw_sessions: Vec<HailSession>,
    #[serde(default)]
    validation_reports: Vec<serde_json::Value>,
}

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

fn load_normalized_session_body(db: &LocalDb, session_id: &str) -> DesktopApiResult<String> {
    let source_path = db.get_session_source_path(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_source_path_failed",
            500,
            "failed to resolve session source path",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;

    if let Some(cached) = db.get_cached_body(session_id).map_err(|error| {
        desktop_error(
            "desktop.session_cache_read_failed",
            500,
            "failed to read cached session body",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
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
                    Some(json!({ "cause": error.to_string(), "session_id": session_id })),
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
        Some(json!({ "session_id": session_id })),
    ))
}

fn canonicalize_summaries(summaries: &[HandoffSummary]) -> DesktopApiResult<String> {
    let mut sorted = summaries
        .iter()
        .map(|summary| {
            serde_json::to_value(summary)
                .map(|value| (summary.source_session_id.clone(), value))
                .map_err(|error| {
                    desktop_error(
                        "desktop.handoff_serialize_failed",
                        500,
                        "failed to serialize handoff summary",
                        Some(json!({ "cause": error.to_string() })),
                    )
                })
        })
        .collect::<DesktopApiResult<Vec<_>>>()?;
    sorted.sort_by(|left, right| left.0.cmp(&right.0));

    let mut out = String::new();
    for (_session_id, value) in sorted {
        let line = serde_json::to_string(&value).map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize canonical handoff line",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn artifact_root_for_cwd(cwd: &Path) -> DesktopApiResult<PathBuf> {
    if let Some(repo_root) = find_repo_root(cwd) {
        return Ok(repo_root.join(".opensession").join("artifacts"));
    }
    let global_objects_root = global_store_root().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve global object store",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;
    let parent = global_objects_root.parent().ok_or_else(|| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "invalid global object store path",
            Some(json!({ "path": global_objects_root })),
        )
    })?;
    Ok(parent.join("artifacts"))
}

fn is_valid_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn artifact_path_for_hash(root: &Path, hash: &str) -> DesktopApiResult<PathBuf> {
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }
    Ok(root
        .join("sha256")
        .join(&hash[0..2])
        .join(&hash[2..4])
        .join(format!("{hash}.json")))
}

fn validate_pin_alias(alias: &str) -> DesktopApiResult<()> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias cannot be empty",
            Some(json!({ "alias": alias })),
        ));
    }
    if !trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-')
    {
        return Err(desktop_error(
            "desktop.handoff_invalid_alias",
            400,
            "pin alias contains invalid characters",
            Some(json!({ "alias": alias })),
        ));
    }
    Ok(())
}

fn pin_path_for_alias(root: &Path, alias: &str) -> DesktopApiResult<PathBuf> {
    validate_pin_alias(alias)?;
    Ok(root.join("pins").join(alias))
}

fn store_handoff_artifact_record(
    record: &DesktopHandoffArtifactRecord,
    cwd: &Path,
) -> DesktopApiResult<()> {
    let root = artifact_root_for_cwd(cwd)?;
    let path = artifact_path_for_hash(&root, &record.sha256)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff artifact directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }
    if !path.exists() {
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to serialize handoff artifact record",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;
        std::fs::write(&path, bytes).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store handoff artifact",
                Some(json!({ "cause": error.to_string(), "path": path })),
            )
        })?;
    }
    Ok(())
}

fn set_handoff_pin(alias: &str, hash: &str, cwd: &Path) -> DesktopApiResult<()> {
    validate_pin_alias(alias)?;
    if !is_valid_sha256(hash) {
        return Err(desktop_error(
            "desktop.handoff_invalid_hash",
            400,
            "invalid artifact hash",
            Some(json!({ "hash": hash })),
        ));
    }

    let root = artifact_root_for_cwd(cwd)?;
    let pin_path = pin_path_for_alias(&root, alias)?;
    if let Some(parent) = pin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to prepare handoff pin directory",
                Some(json!({ "cause": error.to_string(), "path": parent })),
            )
        })?;
    }

    std::fs::write(&pin_path, format!("{hash}\n")).map_err(|error| {
        desktop_error(
            "desktop.handoff_store_failed",
            500,
            "failed to write handoff pin alias",
            Some(json!({ "cause": error.to_string(), "path": pin_path, "alias": alias })),
        )
    })
}

fn build_handoff_artifact_record(
    normalized_session: &str,
    session: HailSession,
    pin_latest: bool,
    cwd: &Path,
) -> DesktopApiResult<DesktopHandoffBuildResponse> {
    let summaries = vec![HandoffSummary::from_session(&session)];
    let reports = validate_handoff_summaries(&summaries);
    let has_error_level = reports.iter().any(|report| {
        report
            .findings
            .iter()
            .any(|finding| finding.severity == "error")
    });
    if has_error_level {
        return Err(desktop_error(
            "desktop.handoff_validation_failed",
            422,
            "handoff validation failed with error-level findings",
            Some(json!({ "reports": reports })),
        ));
    }

    let canonical_jsonl = canonicalize_summaries(&summaries)?;
    let artifact_hash = sha256_hex(canonical_jsonl.as_bytes());

    let source_object =
        store_local_object(normalized_session.as_bytes(), cwd).map_err(|error| {
            desktop_error(
                "desktop.handoff_store_failed",
                500,
                "failed to store canonical source object for handoff",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let validation_reports = reports
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            desktop_error(
                "desktop.handoff_serialize_failed",
                500,
                "failed to serialize handoff validation report",
                Some(json!({ "cause": error.to_string() })),
            )
        })?;

    let mut deduped_source_uris = BTreeSet::new();
    deduped_source_uris.insert(source_object.uri.to_string());

    let record = DesktopHandoffArtifactRecord {
        version: HANDOFF_RECORD_VERSION.to_string(),
        sha256: artifact_hash.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_uris: deduped_source_uris.into_iter().collect(),
        canonical_jsonl,
        raw_sessions: vec![session],
        validation_reports,
    };
    store_handoff_artifact_record(&record, cwd)?;

    if pin_latest {
        set_handoff_pin(HANDOFF_LATEST_PIN_ALIAS, &artifact_hash, cwd)?;
    }

    let artifact_uri = SourceUri::Artifact {
        sha256: artifact_hash,
    }
    .to_string();

    Ok(DesktopHandoffBuildResponse {
        artifact_uri,
        pinned_alias: pin_latest.then_some(HANDOFF_LATEST_PIN_ALIAS.to_string()),
    })
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
    load_normalized_session_body(&db, &id)
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
    let normalized_session = load_normalized_session_body(&db, &session_id)?;
    let session = HailSession::from_jsonl(&normalized_session).map_err(|error| {
        desktop_error(
            "desktop.handoff_parse_failed",
            422,
            "failed to decode normalized session for handoff build",
            Some(json!({ "cause": error.to_string(), "session_id": session_id })),
        )
    })?;
    let cwd = std::env::current_dir().map_err(|error| {
        desktop_error(
            "desktop.handoff_store_unavailable",
            500,
            "failed to resolve current workspace directory",
            Some(json!({ "cause": error.to_string() })),
        )
    })?;

    build_handoff_artifact_record(&normalized_session, session, request.pin_latest, &cwd)
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
        artifact_path_for_hash, build_handoff_artifact_record, build_local_filter,
        canonicalize_summaries, desktop_get_contract_version, map_link_type,
        normalize_session_body_to_hail_jsonl, session_summary_from_local_row, validate_pin_alias,
        DesktopSessionListQuery,
    };
    use opensession_core::handoff::HandoffSummary;
    use opensession_core::trace::{Agent, Session as HailSession};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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
        assert_eq!(
            payload.version,
            opensession_api::DESKTOP_IPC_CONTRACT_VERSION
        );
    }

    #[test]
    fn handoff_canonicalization_orders_by_session_id() {
        let mut session_b = HailSession::new(
            "session-b".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session_b.recompute_stats();
        let mut session_a = HailSession::new(
            "session-a".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session_a.recompute_stats();

        let summaries = vec![
            HandoffSummary::from_session(&session_b),
            HandoffSummary::from_session(&session_a),
        ];
        let canonical = canonicalize_summaries(&summaries).expect("canonicalize summaries");
        let first_line = canonical.lines().next().expect("canonical line");
        assert!(first_line.contains("\"source_session_id\":\"session-a\""));
    }

    #[test]
    fn handoff_pin_alias_validation_rejects_spaces() {
        assert!(validate_pin_alias("latest").is_ok());
        assert!(validate_pin_alias("bad alias").is_err());
    }

    #[test]
    fn artifact_path_rejects_invalid_hash() {
        assert!(artifact_path_for_hash(Path::new("/tmp"), "abc").is_err());
    }

    #[test]
    fn handoff_build_writes_artifact_record_and_pin() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let repo_root = std::env::temp_dir().join(format!("opensession-desktop-handoff-{unique}"));
        let git_dir = repo_root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("create repo .git");

        let mut session = HailSession::new(
            "session-handoff-test".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.recompute_stats();
        let normalized = session.to_jsonl().expect("serialize session");

        let response =
            build_handoff_artifact_record(&normalized, session, true, &repo_root).expect("build");
        let hash = response
            .artifact_uri
            .strip_prefix("os://artifact/")
            .expect("artifact uri prefix");
        assert_eq!(hash.len(), 64);
        assert_eq!(response.pinned_alias.as_deref(), Some("latest"));

        let artifact_path = PathBuf::from(repo_root.join(".opensession").join("artifacts"))
            .join("sha256")
            .join(&hash[0..2])
            .join(&hash[2..4])
            .join(format!("{hash}.json"));
        assert!(artifact_path.exists());

        let pin_path = repo_root
            .join(".opensession")
            .join("artifacts")
            .join("pins")
            .join("latest");
        let pin_hash = std::fs::read_to_string(&pin_path).expect("read pin hash");
        assert_eq!(pin_hash.trim(), hash);

        let _ = std::fs::remove_dir_all(&repo_root);
    }
}
