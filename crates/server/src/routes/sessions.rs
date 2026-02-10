use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    db, SessionDetail, SessionListQuery, SessionListResponse, SessionSummary, UploadResponse,
};

use opensession_core::extract::{extract_first_user_text, extract_user_texts, truncate_str};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

// ---------------------------------------------------------------------------
// Upload session
// ---------------------------------------------------------------------------

/// Server-side upload request uses the strongly-typed Session.
#[derive(serde::Deserialize)]
pub struct UploadRequest {
    pub session: opensession_core::Session,
    pub team_id: String,
}

pub async fn upload_session(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), Response> {
    let session = &req.session;

    // Verify user is a member of the team
    {
        let conn = db.conn();
        let is_member: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
                rusqlite::params![&req.team_id, &user.user_id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !is_member {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "not a member of this team"})),
            )
                .into_response());
        }
    }

    // Serialize body to HAIL JSONL
    let body_jsonl = session.to_jsonl().map_err(|e| {
        tracing::error!("serialize session body: {e}");
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "failed to serialize session"})),
        )
            .into_response()
    })?;

    let session_id = Uuid::new_v4().to_string();

    let storage_key = db
        .write_body(&session_id, body_jsonl.as_bytes())
        .map_err(|e| {
            tracing::error!("write body: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to store session body"})),
            )
                .into_response()
        })?;

    let tags = if session.context.tags.is_empty() {
        None
    } else {
        Some(session.context.tags.join(","))
    };
    let created_at = session.context.created_at.to_rfc3339();

    // Auto-extract title and description from first user messages if empty
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
            &title,
            &description,
            &tags,
            &created_at,
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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to store session"})),
        )
            .into_response()
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
) -> Result<Json<SessionListResponse>, Response> {
    let conn = db.conn();
    let per_page = q.per_page.clamp(1, 100);
    let offset = (q.page.saturating_sub(1)) * per_page;

    // Build dynamic query
    let mut where_clauses = vec!["(s.event_count > 0 OR s.message_count > 0)".to_string()];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1u32;

    if let Some(ref tool) = q.tool {
        where_clauses.push(format!("s.tool = ?{param_idx}"));
        params.push(Box::new(tool.clone()));
        param_idx += 1;
    }

    if let Some(ref team_id) = q.team_id {
        where_clauses.push(format!("s.team_id = ?{param_idx}"));
        params.push(Box::new(team_id.clone()));
        param_idx += 1;
    }

    // Search: use LIKE for flexible partial matching on title, description, tags
    if let Some(ref search) = q.search {
        let like_param = format!("%{search}%");
        where_clauses.push(format!(
            "(s.title LIKE ?{pi} OR s.description LIKE ?{pi2} OR s.tags LIKE ?{pi3})",
            pi = param_idx,
            pi2 = param_idx + 1,
            pi3 = param_idx + 2,
        ));
        params.push(Box::new(like_param.clone()));
        params.push(Box::new(like_param.clone()));
        params.push(Box::new(like_param));
        param_idx += 3;
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
            where_clauses.push(format!("s.created_at >= datetime('now', ?{param_idx})"));
            params.push(Box::new(interval.to_string()));
            param_idx += 1;
        }
    }

    let where_str = where_clauses.join(" AND ");

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM sessions s WHERE {where_str}");
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql).map_err(internal_error)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        stmt.query_row(param_refs.as_slice(), |row| row.get(0))
            .map_err(internal_error)?
    };

    // Sort order
    let order_clause = match q.sort.as_deref() {
        Some("popular") => "s.message_count DESC, s.uploaded_at DESC",
        Some("longest") => "s.duration_seconds DESC, s.uploaded_at DESC",
        _ => "s.uploaded_at DESC", // "recent" or default
    };

    // Fetch page
    let select_sql = format!(
        "SELECT {} \
         FROM sessions s \
         LEFT JOIN users u ON u.id = s.user_id \
         WHERE {where_str} \
         ORDER BY {order_clause} \
         LIMIT ?{param_idx} OFFSET ?{}",
        db::SESSION_COLUMNS,
        param_idx + 1,
    );
    params.push(Box::new(per_page as i64));
    params.push(Box::new(offset as i64));

    let mut stmt = conn.prepare(&select_sql).map_err(internal_error)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                user_id: row.get(1)?,
                nickname: row.get(2)?,
                team_id: row.get(3)?,
                tool: row.get(4)?,
                agent_provider: row.get(5)?,
                agent_model: row.get(6)?,
                title: row.get(7)?,
                description: row.get(8)?,
                tags: row.get(9)?,
                created_at: row.get(10)?,
                uploaded_at: row.get(11)?,
                message_count: row.get(12)?,
                task_count: row.get(13)?,
                event_count: row.get(14)?,
                duration_seconds: row.get(15)?,
                total_input_tokens: row.get(16)?,
                total_output_tokens: row.get(17)?,
            })
        })
        .map_err(internal_error)?;

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
) -> Result<Json<SessionDetail>, Response> {
    let conn = db.conn();

    let summary = conn
        .query_row(&db::SESSION_GET, [&id], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                user_id: row.get(1)?,
                nickname: row.get(2)?,
                team_id: row.get(3)?,
                tool: row.get(4)?,
                agent_provider: row.get(5)?,
                agent_model: row.get(6)?,
                title: row.get(7)?,
                description: row.get(8)?,
                tags: row.get(9)?,
                created_at: row.get(10)?,
                uploaded_at: row.get(11)?,
                message_count: row.get(12)?,
                task_count: row.get(13)?,
                event_count: row.get(14)?,
                duration_seconds: row.get(15)?,
                total_input_tokens: row.get(16)?,
                total_output_tokens: row.get(17)?,
            })
        })
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        })?;

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
) -> Result<Response, Response> {
    let conn = db.conn();

    let storage_key: String = conn
        .query_row(
            "SELECT body_storage_key FROM sessions WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        })?;

    drop(conn);

    let body = db.read_body(&storage_key).map_err(|e| {
        tracing::error!("read body: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to read session body"})),
        )
            .into_response()
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

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("db error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal server error"})),
    )
        .into_response()
}
