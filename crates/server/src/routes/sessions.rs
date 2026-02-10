use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    db, SessionDetail, SessionListQuery, SessionListResponse, SessionSummary, UploadResponse,
};

use opensession_core::extract::{extract_first_user_text, extract_user_texts, truncate_str};

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
}

struct SessionMetadata {
    tags: Option<String>,
    created_at: String,
    title: Option<String>,
    description: Option<String>,
}

fn extract_session_metadata(session: &opensession_core::Session) -> SessionMetadata {
    let tags = if session.context.tags.is_empty() {
        None
    } else {
        Some(session.context.tags.join(","))
    };

    let title = session
        .context
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| extract_first_user_text(session).map(|t| truncate_str(&t, 80)));

    let description = session
        .context
        .description
        .clone()
        .filter(|d| !d.is_empty())
        .or_else(|| extract_user_texts(session, 3).map(|t| truncate_str(&t, 500)));

    SessionMetadata {
        tags,
        created_at: session.context.created_at.to_rfc3339(),
        title,
        description,
    }
}

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

    let meta = extract_session_metadata(session);

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

pub async fn list_sessions(
    State(db): State<Db>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<SessionListResponse>, ApiErr> {
    let conn = db.conn();
    let per_page = q.per_page.clamp(1, 100);
    let offset = (q.page.saturating_sub(1)) * per_page;

    // Build dynamic query
    let mut where_clauses = vec!["(s.event_count > 0 OR s.message_count > 0)".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref tool) = q.tool {
        let idx = params.len() + 1;
        params.push(Box::new(tool.clone()));
        where_clauses.push(format!("s.tool = ?{idx}"));
    }

    if let Some(ref team_id) = q.team_id {
        let idx = params.len() + 1;
        params.push(Box::new(team_id.clone()));
        where_clauses.push(format!("s.team_id = ?{idx}"));
    }

    // Search: use LIKE for flexible partial matching on title, description, tags
    if let Some(ref search) = q.search {
        let like_param = format!("%{search}%");
        let base = params.len() + 1;
        params.push(Box::new(like_param.clone()));
        params.push(Box::new(like_param.clone()));
        params.push(Box::new(like_param));
        where_clauses.push(format!(
            "(s.title LIKE ?{base} OR s.description LIKE ?{} OR s.tags LIKE ?{})",
            base + 1,
            base + 2,
        ));
    }

    // Time range filter
    if let Some(ref time_range) = q.time_range {
        let interval = match time_range.as_str() {
            "24h" => Some("-1 day"),
            "7d" => Some("-7 days"),
            "30d" => Some("-30 days"),
            _ => None, // "all" or unrecognized
        };
        if let Some(interval) = interval {
            let idx = params.len() + 1;
            params.push(Box::new(interval.to_string()));
            where_clauses.push(format!("s.created_at >= datetime('now', ?{idx})"));
        }
    }

    let where_str = where_clauses.join(" AND ");

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM sessions s WHERE {where_str}");
    let total: i64 = {
        let mut stmt = conn
            .prepare(&count_sql)
            .map_err(ApiErr::from_db("prepare count"))?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        stmt.query_row(param_refs.as_slice(), |row| row.get(0))
            .map_err(ApiErr::from_db("count sessions"))?
    };

    // Sort order
    let order_clause = match q.sort.as_deref() {
        Some("popular") => "s.message_count DESC, s.uploaded_at DESC",
        Some("longest") => "s.duration_seconds DESC, s.uploaded_at DESC",
        _ => "s.uploaded_at DESC", // "recent" or default
    };

    // Fetch page
    let limit_idx = params.len() + 1;
    params.push(Box::new(per_page as i64));
    params.push(Box::new(offset as i64));

    let select_sql = format!(
        "SELECT {} \
         FROM sessions s \
         LEFT JOIN users u ON u.id = s.user_id \
         WHERE {where_str} \
         ORDER BY {order_clause} \
         LIMIT ?{limit_idx} OFFSET ?{}",
        db::SESSION_COLUMNS,
        limit_idx + 1,
    );

    let mut stmt = conn
        .prepare(&select_sql)
        .map_err(ApiErr::from_db("prepare list"))?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), session_from_row)
        .map_err(ApiErr::from_db("list sessions"))?;

    let sessions: Vec<SessionSummary> = rows.filter_map(|r| r.ok()).collect();

    Ok(Json(SessionListResponse {
        sessions,
        total,
        page: q.page,
        per_page,
    }))
}

// ---------------------------------------------------------------------------
// Get session detail
// ---------------------------------------------------------------------------

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

    Ok(Json(SessionDetail { summary, team_name }))
}

// ---------------------------------------------------------------------------
// Get raw session body
// ---------------------------------------------------------------------------

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
