use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use opensession_api_types::service::ParamValue;
use opensession_api_types::{
    db, service, SessionDetail, SessionLink, SessionListQuery, SessionListResponse, SessionSummary,
    UploadResponse,
};

use opensession_core::extract;

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, Db};

// ---------------------------------------------------------------------------
// Upload session
// ---------------------------------------------------------------------------

/// Server-side upload request uses the strongly-typed Session.
#[derive(serde::Deserialize)]
pub struct UploadRequest {
    pub session: opensession_core::Session,
    pub team_id: String,
    #[serde(default)]
    pub linked_session_ids: Option<Vec<String>>,
}

/// POST /api/sessions — upload a new session (requires team membership).
pub async fn upload_session(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), ApiErr> {
    let session = &req.session;

    if !db.is_team_member(&req.team_id, &user.user_id) {
        return Err(ApiErr::forbidden("not a member of this team"));
    }

    let body_jsonl = session.to_jsonl().map_err(|e| {
        tracing::error!("serialize session body: {e}");
        ApiErr::bad_request("failed to serialize session")
    })?;

    let session_id = Uuid::new_v4().to_string();

    let storage_key = db
        .write_body(&session_id, body_jsonl.as_bytes())
        .map_err(|e| {
            tracing::error!("write body: {e}");
            ApiErr::internal("failed to store session body")
        })?;

    let meta = extract::extract_upload_metadata(session);

    let conn = db.conn();
    conn.execute(
        db::SESSION_INSERT,
        rusqlite::params![
            &session_id,
            &user.user_id,
            &req.team_id,
            &session.agent.tool,
            &session.agent.provider,
            &session.agent.model,
            &meta.title,
            &meta.description,
            &meta.tags,
            &meta.created_at,
            session.stats.message_count as i64,
            session.stats.task_count as i64,
            session.stats.event_count as i64,
            session.stats.duration_seconds as i64,
            session.stats.total_input_tokens as i64,
            session.stats.total_output_tokens as i64,
            &storage_key,
        ],
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
        let _ = conn.execute(
            db::SESSION_LINK_INSERT,
            rusqlite::params![&session_id, linked_id, "handoff"],
        );
    }

    // Update FTS
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

/// Convert shared [`ParamValue`] to rusqlite bind params.
fn to_rusqlite_params(params: &[ParamValue]) -> Vec<Box<dyn rusqlite::types::ToSql>> {
    params
        .iter()
        .map(|p| -> Box<dyn rusqlite::types::ToSql> {
            match p {
                ParamValue::Text(s) => Box::new(s.clone()),
                ParamValue::Int(n) => Box::new(*n),
            }
        })
        .collect()
}

/// GET /api/sessions — list sessions (public, paginated, filtered).
pub async fn list_sessions(
    State(db): State<Db>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<SessionListResponse>, ApiErr> {
    let built = service::build_session_list_query(&q);
    let conn = db.conn();

    // Count total
    let total: i64 = {
        let params = to_rusqlite_params(&built.count_params);
        let mut stmt = conn
            .prepare(&built.count_sql)
            .map_err(ApiErr::from_db("prepare count"))?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        stmt.query_row(refs.as_slice(), |row| row.get(0))
            .map_err(ApiErr::from_db("count sessions"))?
    };

    // Fetch page
    let params = to_rusqlite_params(&built.select_params);
    let mut stmt = conn
        .prepare(&built.select_sql)
        .map_err(ApiErr::from_db("prepare list"))?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), session_from_row)
        .map_err(ApiErr::from_db("list sessions"))?;

    let sessions: Vec<SessionSummary> = rows.filter_map(|r| r.ok()).collect();

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

    let summary = conn
        .query_row(&db::SESSION_GET, [&id], session_from_row)
        .map_err(|_| ApiErr::not_found("session not found"))?;

    let team_name: Option<String> = conn
        .query_row(
            "SELECT name FROM teams WHERE id = ?1",
            [&summary.team_id],
            |row| row.get(0),
        )
        .ok();

    // Fetch linked sessions
    let linked_sessions = {
        let mut stmt = conn
            .prepare(db::SESSION_LINKS_BY_SESSION)
            .map_err(ApiErr::from_db("prepare session_links"))?;
        let rows = stmt
            .query_map([&id], |row| {
                Ok(SessionLink {
                    session_id: row.get(0)?,
                    linked_session_id: row.get(1)?,
                    link_type: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(ApiErr::from_db("query session_links"))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(Json(SessionDetail {
        summary,
        team_name,
        linked_sessions,
    }))
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

    let storage_key: String = conn
        .query_row(
            "SELECT body_storage_key FROM sessions WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )
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
