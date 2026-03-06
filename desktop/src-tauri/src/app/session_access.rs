use crate::app::session_query::{SearchMode, build_local_filter_with_mode};
use crate::app::vector::list_sessions_with_vector_rank;
use crate::{DesktopApiResult, desktop_error, open_local_db};
use opensession_api::{
    DesktopSessionListQuery, LinkType, SessionDetail, SessionLink, SessionListResponse,
    SessionRepoListResponse, SessionSummary,
};
use opensession_core::session::{is_auxiliary_session, working_directory};
use opensession_core::trace::Session as HailSession;
use opensession_git_native::extract_git_context;
use opensession_local_db::{LocalDb, LocalSessionLink, LocalSessionRow};
use opensession_parsers::{
    discover::discover_for_tool, ingest::preview_parse_bytes, parse_with_default_parsers,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const FORCE_REFRESH_MAX_DISCOVERY_PATHS: usize = 240;

pub(crate) fn force_refresh_discovery_tools() -> &'static [&'static str] {
    // Cursor workspace DBs are high-volume and often metadata-only. Exclude them from the
    // synchronous force-refresh path so recent sessions show up immediately.
    &["codex", "claude-code", "opencode", "cline", "amp", "gemini"]
}

fn force_refresh_discovered_paths() -> Vec<PathBuf> {
    let mut unique_paths = BTreeSet::new();
    for tool in force_refresh_discovery_tools() {
        for path in discover_for_tool(tool) {
            if path.exists() {
                unique_paths.insert(path);
            }
        }
    }

    let mut paths: Vec<PathBuf> = unique_paths.into_iter().collect();
    paths.sort_by(|left, right| {
        let left_modified = fs::metadata(left).and_then(|meta| meta.modified()).ok();
        let right_modified = fs::metadata(right).and_then(|meta| meta.modified()).ok();
        right_modified
            .cmp(&left_modified)
            .then_with(|| left.cmp(right))
    });
    paths.truncate(FORCE_REFRESH_MAX_DISCOVERY_PATHS);
    paths
}

fn refresh_local_session_index(db: &LocalDb) {
    let mut parse_errors = 0usize;
    let mut upsert_errors = 0usize;
    let mut upserted = 0usize;

    for path in force_refresh_discovered_paths() {
        let parsed = match parse_with_default_parsers(&path) {
            Ok(session) => session,
            Err(error) => {
                parse_errors = parse_errors.saturating_add(1);
                if parse_errors <= 5 {
                    eprintln!(
                        "failed to parse discovered session {}: {error}",
                        path.display()
                    );
                }
                continue;
            }
        };
        let Some(session) = parsed else {
            continue;
        };
        if is_auxiliary_session(&session) {
            continue;
        }

        let git = working_directory(&session)
            .map(extract_git_context)
            .unwrap_or_default();
        let local_git = opensession_local_db::git::GitContext {
            remote: git.remote.clone(),
            branch: git.branch.clone(),
            commit: git.commit.clone(),
            repo_name: git.repo_name.clone(),
        };
        let path_str = path.to_string_lossy().to_string();

        if let Err(error) = db.upsert_local_session(&session, &path_str, &local_git) {
            upsert_errors = upsert_errors.saturating_add(1);
            if upsert_errors <= 5 {
                eprintln!(
                    "failed to upsert discovered session {}: {error}",
                    path.display()
                );
            }
            continue;
        }

        upserted = upserted.saturating_add(1);
    }

    if parse_errors > 5 {
        eprintln!(
            "force refresh parse errors suppressed: {} additional failures",
            parse_errors - 5
        );
    }
    if upsert_errors > 5 {
        eprintln!(
            "force refresh upsert errors suppressed: {} additional failures",
            upsert_errors - 5
        );
    }
    eprintln!("force refresh reindex complete: upserted={upserted}");
}

pub(crate) fn session_summary_from_local_row(row: LocalSessionRow) -> SessionSummary {
    session_summary_from_local_row_with_score(
        row,
        0,
        opensession_core::scoring::DEFAULT_SCORE_PLUGIN,
    )
}

pub(crate) fn session_summary_from_local_row_with_score(
    row: LocalSessionRow,
    session_score: i64,
    score_plugin: &str,
) -> SessionSummary {
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
        session_score,
        score_plugin: score_plugin.to_string(),
    }
}

pub(crate) fn map_link_type(raw: &str) -> LinkType {
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

pub(crate) fn normalize_session_body_to_hail_jsonl(
    body: &str,
    source_path: Option<&str>,
) -> DesktopApiResult<String> {
    if let Ok(session) = HailSession::from_jsonl(body) {
        return session_to_hail_jsonl(session);
    }

    if let Ok(session) = serde_json::from_str::<HailSession>(body) {
        return session_to_hail_jsonl(session);
    }

    if let Some(path_text) = source_path {
        let path = Path::new(path_text);
        if let Ok(Some(session)) = parse_with_default_parsers(path) {
            return session_to_hail_jsonl(session);
        }
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

fn read_source_session_text(source_path: &str) -> DesktopApiResult<String> {
    std::fs::read_to_string(source_path).map_err(|error| {
        desktop_error(
            "desktop.session_source_unavailable",
            404,
            format!("session source file is unavailable ({source_path})"),
            Some(json!({ "cause": error.to_string(), "source_path": source_path })),
        )
    })
}

pub(crate) fn load_normalized_session_body(
    db: &LocalDb,
    session_id: &str,
) -> DesktopApiResult<String> {
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
        let source_body = read_source_session_text(&source_path)?;
        let normalized = normalize_session_body_to_hail_jsonl(&source_body, Some(&source_path))?;
        if let Err(error) = db.cache_body(session_id, source_body.as_bytes()) {
            eprintln!("failed to cache normalized session source for {session_id}: {error}");
        }
        return Ok(normalized);
    }

    Err(desktop_error(
        "desktop.session_body_not_found",
        404,
        "session body not found in local cache",
        Some(json!({ "session_id": session_id })),
    ))
}

#[tauri::command]
pub(crate) fn desktop_list_sessions(
    query: Option<DesktopSessionListQuery>,
) -> DesktopApiResult<SessionListResponse> {
    let db = open_local_db()?;
    let query = query.unwrap_or_default();
    if query.force_refresh.unwrap_or(false) {
        refresh_local_session_index(&db);
    }
    let (filter, page, per_page, search_mode) = build_local_filter_with_mode(query);

    if search_mode == SearchMode::Vector {
        let vector_query = filter.search.clone().unwrap_or_default();
        return list_sessions_with_vector_rank(&db, &filter, &vector_query, page, per_page);
    }

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
pub(crate) fn desktop_list_repos() -> DesktopApiResult<SessionRepoListResponse> {
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
pub(crate) fn desktop_get_session_detail(id: String) -> DesktopApiResult<SessionDetail> {
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
pub(crate) fn desktop_get_session_raw(id: String) -> DesktopApiResult<String> {
    let db = open_local_db()?;
    load_normalized_session_body(&db, &id)
}
