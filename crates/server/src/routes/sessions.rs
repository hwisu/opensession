use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use opensession_api::{
    db, saturating_i64, LinkType, OkResponse, SessionDetail, SessionLink, SessionListQuery,
    SessionListResponse, SessionSummary, UploadRequest, UploadResponse,
};

use opensession_core::extract;
use opensession_core::scoring::SessionScoreRegistry;

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, sq_execute, sq_query_map, sq_query_row, Db};
use crate::AppConfig;

const PUBLIC_LIST_CACHE_CONTROL: &str = "public, max-age=30, stale-while-revalidate=60";

#[derive(Debug, PartialEq, Eq)]
enum RawBodySource {
    LocalStorage(String),
    RedirectUrl(String),
}

fn resolve_raw_body_source(
    body_storage_key: String,
    body_url: Option<String>,
) -> Result<RawBodySource, ApiErr> {
    if let Some(url) = body_url.map(|v| v.trim().to_string()) {
        if !url.is_empty() {
            return Ok(RawBodySource::RedirectUrl(url));
        }
    }

    let key = body_storage_key.trim().to_string();
    if key.is_empty() {
        return Err(ApiErr::not_found("session body not found"));
    }

    Ok(RawBodySource::LocalStorage(key))
}

// ---------------------------------------------------------------------------
// Upload session
// ---------------------------------------------------------------------------

/// POST /api/sessions — upload a new session (authenticated users only).
pub async fn upload_session(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), ApiErr> {
    let session = &req.session;
    let team_id = "local";

    let body_jsonl = session.to_jsonl().map_err(|e| {
        tracing::error!("serialize session body: {e}");
        ApiErr::bad_request("failed to serialize session")
    })?;

    let session_id = Uuid::new_v4().to_string();

    // If body_url is provided (external git storage), skip local body storage
    let (storage_key, effective_body_url) = if req.body_url.is_some() {
        (String::new(), req.body_url.as_deref())
    } else {
        let key = db
            .write_body(&session_id, body_jsonl.as_bytes())
            .map_err(|e| {
                tracing::error!("write body: {e}");
                ApiErr::internal("failed to store session body")
            })?;
        (key, None)
    };

    let meta = extract::extract_upload_metadata(session);
    let score_registry = SessionScoreRegistry::default();
    let requested_score_plugin = req
        .score_plugin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let env_score_plugin = std::env::var(opensession_api::deploy::ENV_SESSION_SCORE_PLUGIN)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let selected_score_plugin = requested_score_plugin
        .or(env_score_plugin.as_deref())
        .unwrap_or(opensession_core::scoring::DEFAULT_SCORE_PLUGIN);
    let session_score = match score_registry.score_with(selected_score_plugin, session) {
        Ok(result) => result,
        Err(err) => {
            if requested_score_plugin.is_some() {
                return Err(ApiErr::bad_request(err.to_string()));
            }
            let fallback = score_registry.score_default(session).map_err(|score_err| {
                tracing::error!("fallback score plugin failed: {score_err}");
                ApiErr::internal("failed to compute session score")
            })?;
            tracing::warn!(
                "score plugin '{}' unavailable; fallback to '{}': {}",
                selected_score_plugin,
                fallback.plugin,
                err
            );
            fallback
        }
    };

    let conn = db.conn();
    sq_execute(
        &conn,
        db::sessions::insert(&db::sessions::InsertParams {
            id: &session_id,
            user_id: &user.user_id,
            team_id,
            tool: &session.agent.tool,
            agent_provider: &session.agent.provider,
            agent_model: &session.agent.model,
            title: meta.title.as_deref().unwrap_or(""),
            description: meta.description.as_deref().unwrap_or(""),
            tags: meta.tags.as_deref().unwrap_or(""),
            created_at: &meta.created_at,
            message_count: saturating_i64(session.stats.message_count),
            task_count: saturating_i64(session.stats.task_count),
            event_count: saturating_i64(session.stats.event_count),
            duration_seconds: saturating_i64(session.stats.duration_seconds),
            total_input_tokens: saturating_i64(session.stats.total_input_tokens),
            total_output_tokens: saturating_i64(session.stats.total_output_tokens),
            body_storage_key: &storage_key,
            body_url: effective_body_url,
            git_remote: req.git_remote.as_deref(),
            git_branch: req.git_branch.as_deref(),
            git_commit: req.git_commit.as_deref(),
            git_repo_name: req.git_repo_name.as_deref(),
            pr_number: req.pr_number,
            pr_url: req.pr_url.as_deref(),
            working_directory: meta.working_directory.as_deref(),
            files_modified: meta.files_modified.as_deref(),
            files_read: meta.files_read.as_deref(),
            has_errors: meta.has_errors,
            max_active_agents: saturating_i64(opensession_core::agent_metrics::max_active_agents(
                session,
            ) as u64),
            session_score: session_score.score,
            score_plugin: &session_score.plugin,
        }),
    )
    .map_err(|e| {
        tracing::error!("insert session: {e}");
        ApiErr::internal("failed to store session")
    })?;

    // Insert session links: prefer explicit linked_session_ids, fall back to context.related_session_ids
    let linked_ids: Vec<String> = req
        .linked_session_ids
        .unwrap_or_default()
        .into_iter()
        .chain(session.context.related_session_ids.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for linked_id in &linked_ids {
        let _ = sq_execute(
            &conn,
            db::sessions::insert_link(&session_id, linked_id, LinkType::Handoff),
        );
    }

    // Update FTS (server-specific; D1 does not support FTS)
    let _ = sq_execute(&conn, db::sessions::insert_fts(&session_id));

    let base_url = std::env::var("BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("OPENSESSION_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "http://localhost:3000".into());
    let url = format!("{base_url}/session/{session_id}");

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            id: session_id,
            url,
            session_score: session_score.score,
            score_plugin: session_score.plugin,
        }),
    ))
}

// ---------------------------------------------------------------------------
// List sessions
// ---------------------------------------------------------------------------

/// GET /api/sessions — list sessions (public, paginated, filtered).
pub async fn list_sessions(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    auth_user: Result<AuthUser, ApiErr>,
    Query(q): Query<SessionListQuery>,
    headers: HeaderMap,
) -> Result<axum::response::Response, ApiErr> {
    let has_auth_header = headers.get(header::AUTHORIZATION).is_some();
    let is_authenticated = auth_user.is_ok();
    if !can_access_session_list(config.public_feed_enabled, is_authenticated) {
        return Err(ApiErr::unauthorized(
            "public session feed is disabled; authentication required",
        ));
    }

    let built = db::sessions::list(&q);
    let conn = db.conn();

    // Count total
    let total: i64 = sq_query_row(&conn, built.count_query, |row| row.get(0))
        .map_err(ApiErr::from_db("count sessions"))?;

    // Fetch page
    let sessions: Vec<SessionSummary> = sq_query_map(&conn, built.select_query, session_from_row)
        .map_err(ApiErr::from_db("list sessions"))?;

    let mut resp = Json(SessionListResponse {
        sessions,
        total,
        page: built.page,
        per_page: built.per_page,
    })
    .into_response();

    let has_session_cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|cookie| cookie.contains("session="));
    if q.is_public_feed_cacheable(has_auth_header, has_session_cookie) {
        resp.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(PUBLIC_LIST_CACHE_CONTROL),
        );
    }

    Ok(resp)
}

fn can_access_session_list(public_feed_enabled: bool, is_authenticated: bool) -> bool {
    public_feed_enabled || is_authenticated
}

// ---------------------------------------------------------------------------
// Get session detail
// ---------------------------------------------------------------------------

/// GET /api/sessions/:id — get session detail with linked sessions.
pub async fn get_session(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, ApiErr> {
    let conn = db.conn();

    let summary: SessionSummary =
        sq_query_row(&conn, db::sessions::get_by_id(&id), session_from_row)
            .map_err(|_| ApiErr::not_found("session not found"))?;

    // Fetch linked sessions
    let linked_sessions: Vec<SessionLink> =
        sq_query_map(&conn, db::sessions::links_by_session(&id), |row| {
            let lt: String = row.get(2)?;
            Ok(SessionLink {
                session_id: row.get(0)?,
                linked_session_id: row.get(1)?,
                link_type: match lt.as_str() {
                    "related" => LinkType::Related,
                    "parent" => LinkType::Parent,
                    "child" => LinkType::Child,
                    _ => LinkType::Handoff,
                },
                created_at: row.get(3)?,
            })
        })
        .map_err(ApiErr::from_db("query session_links"))?;

    Ok(Json(SessionDetail {
        summary,
        linked_sessions,
    }))
}

// ---------------------------------------------------------------------------
// Delete session
// ---------------------------------------------------------------------------

/// DELETE /api/sessions/:id — delete a session (owner only).
pub async fn delete_session(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();

    let summary: SessionSummary =
        sq_query_row(&conn, db::sessions::get_by_id(&id), session_from_row)
            .map_err(|_| ApiErr::not_found("session not found"))?;
    let _ = summary;

    sq_execute(&conn, db::sessions::delete_links(&id)).map_err(ApiErr::from_db("delete links"))?;
    sq_execute(&conn, db::sessions::delete(&id)).map_err(ApiErr::from_db("delete session"))?;

    // Clean up FTS (server-specific; D1 does not support FTS)
    let _ = sq_execute(&conn, db::sessions::delete_fts(&id));

    Ok(Json(OkResponse { ok: true }))
}

// ---------------------------------------------------------------------------
// Get raw session body
// ---------------------------------------------------------------------------

/// GET /api/sessions/:id/raw — download the full HAIL JSONL body.
pub async fn get_session_raw(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, ApiErr> {
    let conn = db.conn();

    let (body_storage_key, body_url): (String, Option<String>) =
        sq_query_row(&conn, db::sessions::get_storage_info(&id), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| ApiErr::not_found("session not found"))?;

    drop(conn);

    match resolve_raw_body_source(body_storage_key, body_url)? {
        RawBodySource::RedirectUrl(url) => {
            let location = HeaderValue::from_str(&url)
                .map_err(|_| ApiErr::internal("invalid session body URL"))?;
            let mut response = StatusCode::FOUND.into_response();
            response.headers_mut().insert(header::LOCATION, location);
            Ok(response)
        }
        RawBodySource::LocalStorage(storage_key) => {
            let body = db.read_body(&storage_key).map_err(|e| {
                tracing::error!("read body: {e}");
                ApiErr::internal("failed to read session body")
            })?;

            Ok((
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, "application/jsonl"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"session.hail.jsonl\"",
                    ),
                ],
                body,
            )
                .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{can_access_session_list, resolve_raw_body_source, RawBodySource};

    #[test]
    fn session_list_access_rules_follow_public_feed_flag() {
        assert!(can_access_session_list(true, false));
        assert!(can_access_session_list(true, true));
        assert!(can_access_session_list(false, true));
        assert!(!can_access_session_list(false, false));
    }

    #[test]
    fn raw_body_source_prefers_redirect_when_body_url_present() {
        let source = match resolve_raw_body_source(
            "".to_string(),
            Some("https://example.com/a".to_string()),
        ) {
            Ok(source) => source,
            Err(_) => panic!("body_url should resolve to redirect"),
        };
        match source {
            RawBodySource::RedirectUrl(url) => assert_eq!(url, "https://example.com/a"),
            RawBodySource::LocalStorage(_) => panic!("expected redirect source"),
        }
    }

    #[test]
    fn raw_body_source_uses_storage_key_when_no_body_url() {
        let source = match resolve_raw_body_source("abc.hail.jsonl".to_string(), None) {
            Ok(source) => source,
            Err(_) => panic!("storage key should resolve to local storage"),
        };
        match source {
            RawBodySource::LocalStorage(key) => assert_eq!(key, "abc.hail.jsonl"),
            RawBodySource::RedirectUrl(_) => panic!("expected local storage source"),
        }
    }

    #[test]
    fn raw_body_source_rejects_empty_storage_and_body_url() {
        let err = resolve_raw_body_source("".to_string(), Some("   ".to_string()))
            .expect_err("empty storage/body_url should fail");
        let response = axum::response::IntoResponse::into_response(err);
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
}
