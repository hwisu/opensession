use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use opensession_api::{
    db, saturating_i64, LinkType, OkResponse, SessionDetail, SessionLink, SessionListQuery,
    SessionListResponse, SessionSummary, UploadRequest, UploadResponse,
};

use opensession_core::extract;

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, sq_execute, sq_query_map, sq_query_row, Db};

// ---------------------------------------------------------------------------
// Upload session
// ---------------------------------------------------------------------------

/// POST /api/sessions — upload a new session (requires team membership).
pub async fn upload_session(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), ApiErr> {
    let session = &req.session;
    let team_id = req.team_id.as_deref().unwrap_or("personal");

    if !db.is_team_member(team_id, &user.user_id) {
        return Err(ApiErr::forbidden("not a member of this team"));
    }

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

    // Update FTS (Axum-specific, kept inline)
    let _ = conn.execute(
        "INSERT INTO sessions_fts (rowid, title, description, tags)
         SELECT rowid, title, description, tags FROM sessions WHERE id = ?1",
        [&session_id],
    );

    let base_url =
        std::env::var("OPENSESSION_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let url = format!("{base_url}/session/{session_id}");

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            id: session_id,
            url,
        }),
    ))
}

// ---------------------------------------------------------------------------
// List sessions (team-scoped)
// ---------------------------------------------------------------------------

/// GET /api/sessions — list sessions (public, paginated, filtered).
pub async fn list_sessions(
    State(db): State<Db>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<SessionListResponse>, ApiErr> {
    let built = db::sessions::list(&q);
    let conn = db.conn();

    // Count total
    let total: i64 = sq_query_row(&conn, built.count_query, |row| row.get(0))
        .map_err(ApiErr::from_db("count sessions"))?;

    // Fetch page
    let sessions: Vec<SessionSummary> = sq_query_map(&conn, built.select_query, session_from_row)
        .map_err(ApiErr::from_db("list sessions"))?;

    Ok(Json(SessionListResponse {
        sessions,
        total,
        page: built.page,
        per_page: built.per_page,
    }))
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

    let team_name: Option<String> =
        sq_query_row(&conn, db::teams::get_name(&summary.team_id), |row| {
            row.get(0)
        })
        .ok();

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
        team_name,
        linked_sessions,
    }))
}

// ---------------------------------------------------------------------------
// Delete session
// ---------------------------------------------------------------------------

/// DELETE /api/sessions/:id — delete a session (owner only).
pub async fn delete_session(
    State(db): State<Db>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();

    let summary: SessionSummary =
        sq_query_row(&conn, db::sessions::get_by_id(&id), session_from_row)
            .map_err(|_| ApiErr::not_found("session not found"))?;

    if summary.user_id.as_deref() != Some(&auth.user_id) {
        return Err(ApiErr::forbidden("not your session"));
    }

    sq_execute(&conn, db::sessions::delete_links(&id)).map_err(ApiErr::from_db("delete links"))?;
    sq_execute(&conn, db::sessions::delete(&id)).map_err(ApiErr::from_db("delete session"))?;

    // Clean up FTS
    let _ = conn.execute(
        "DELETE FROM sessions_fts WHERE rowid IN (SELECT rowid FROM sessions WHERE id = ?1)",
        [&id],
    );

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

    let storage_key: String =
        sq_query_row(&conn, db::sessions::get_storage_info(&id), |row| row.get(0))
            .map_err(|_| ApiErr::not_found("session not found"))?;

    drop(conn);

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
