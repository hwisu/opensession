use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    GroupRef, SessionDetail, SessionListQuery, SessionListResponse, SessionSummary, UploadResponse,
};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

// ---------------------------------------------------------------------------
// Upload session
// ---------------------------------------------------------------------------

/// Server-side upload request uses the strongly-typed Session (not serde_json::Value).
#[derive(serde::Deserialize)]
pub struct UploadRequest {
    pub session: opensession_core::Session,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub group_ids: Vec<String>,
}

pub async fn upload_session(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>), Response> {
    let session = &req.session;

    // Serialize body to JSON
    let body_json = serde_json::to_vec(session).map_err(|e| {
        tracing::error!("serialize session body: {e}");
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid session JSON"})),
        )
            .into_response()
    })?;

    let session_id = Uuid::new_v4().to_string();

    let storage_key = db.write_body(&session_id, &body_json).map_err(|e| {
        tracing::error!("write body: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to store session body"})),
        )
            .into_response()
    })?;

    let visibility = req.visibility.as_deref().unwrap_or("public");
    let tags = if session.context.tags.is_empty() {
        None
    } else {
        Some(session.context.tags.join(","))
    };
    let created_at = session.context.created_at.to_rfc3339();

    // Auto-extract title and description from first user messages if empty
    let title = session.context.title.clone().filter(|t| !t.is_empty())
        .or_else(|| extract_first_user_text(session).map(|t| truncate_str(&t, 80)));
    let description = session.context.description.clone().filter(|d| !d.is_empty())
        .or_else(|| extract_user_texts(session, 3).map(|t| truncate_str(&t, 500)));

    let conn = db.conn();
    conn.execute(
        "INSERT INTO sessions (id, user_id, tool, agent_provider, agent_model, title, description, tags, visibility, created_at, message_count, task_count, event_count, duration_seconds, body_storage_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            &session_id,
            &user.user_id,
            &session.agent.tool,
            &session.agent.provider,
            &session.agent.model,
            &title,
            &description,
            &tags,
            visibility,
            &created_at,
            session.stats.message_count as i64,
            session.stats.task_count as i64,
            session.stats.event_count as i64,
            session.stats.duration_seconds as i64,
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

    // Link to groups
    for group_id in &req.group_ids {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO session_groups (session_id, group_id) VALUES (?1, ?2)",
            rusqlite::params![&session_id, group_id],
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

    Ok((StatusCode::CREATED, Json(UploadResponse { id: session_id, url })))
}

// ---------------------------------------------------------------------------
// List sessions
// ---------------------------------------------------------------------------

pub async fn list_sessions(
    State(db): State<Db>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<SessionListResponse>, Response> {
    let conn = db.conn();
    let per_page = q.per_page.clamp(1, 100);
    let offset = (q.page.saturating_sub(1)) * per_page;

    // Build dynamic query
    let mut where_clauses = vec![
        "s.visibility = 'public'".to_string(),
        "(s.event_count > 0 OR s.message_count > 0)".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1u32;

    if let Some(ref tool) = q.tool {
        where_clauses.push(format!("s.tool = ?{param_idx}"));
        params.push(Box::new(tool.clone()));
        param_idx += 1;
    }

    if let Some(ref group_id) = q.group_id {
        where_clauses.push(format!(
            "s.id IN (SELECT session_id FROM session_groups WHERE group_id = ?{param_idx})"
        ));
        params.push(Box::new(group_id.clone()));
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
    let count_sql = format!(
        "SELECT COUNT(*) FROM sessions s WHERE {where_str}"
    );
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql).map_err(internal_error)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
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
        "SELECT s.id, s.user_id, u.nickname, s.tool, s.agent_provider, s.agent_model, s.title, s.description, s.tags, s.visibility, s.created_at, s.uploaded_at, s.message_count, s.task_count, s.event_count, s.duration_seconds, u.avatar_url
         FROM sessions s
         LEFT JOIN users u ON u.id = s.user_id
         WHERE {where_str}
         ORDER BY {order_clause}
         LIMIT ?{param_idx} OFFSET ?{}",
        param_idx + 1
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
                tool: row.get(3)?,
                agent_provider: row.get(4)?,
                agent_model: row.get(5)?,
                title: row.get(6)?,
                description: row.get(7)?,
                tags: row.get(8)?,
                visibility: row.get(9)?,
                created_at: row.get(10)?,
                uploaded_at: row.get(11)?,
                message_count: row.get(12)?,
                task_count: row.get(13)?,
                event_count: row.get(14)?,
                duration_seconds: row.get(15)?,
                avatar_url: row.get(16)?,
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
        .query_row(
            "SELECT s.id, s.user_id, u.nickname, s.tool, s.agent_provider, s.agent_model, s.title, s.description, s.tags, s.visibility, s.created_at, s.uploaded_at, s.message_count, s.task_count, s.event_count, s.duration_seconds, u.avatar_url
             FROM sessions s
             LEFT JOIN users u ON u.id = s.user_id
             WHERE s.id = ?1",
            [&id],
            |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    nickname: row.get(2)?,
                    tool: row.get(3)?,
                    agent_provider: row.get(4)?,
                    agent_model: row.get(5)?,
                    title: row.get(6)?,
                    description: row.get(7)?,
                    tags: row.get(8)?,
                    visibility: row.get(9)?,
                    created_at: row.get(10)?,
                    uploaded_at: row.get(11)?,
                    message_count: row.get(12)?,
                    task_count: row.get(13)?,
                    event_count: row.get(14)?,
                    duration_seconds: row.get(15)?,
                    avatar_url: row.get(16)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        })?;

    // Check visibility
    if summary.visibility != "public" {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        )
            .into_response());
    }

    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.name FROM groups g
             INNER JOIN session_groups sg ON sg.group_id = g.id
             WHERE sg.session_id = ?1",
        )
        .map_err(internal_error)?;

    let groups: Vec<GroupRef> = stmt
        .query_map([&id], |row| {
            Ok(GroupRef {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(SessionDetail { summary, groups }))
}

// ---------------------------------------------------------------------------
// Get raw session body
// ---------------------------------------------------------------------------

pub async fn get_session_raw(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Response, Response> {
    let conn = db.conn();

    let (visibility, storage_key): (String, String) = conn
        .query_row(
            "SELECT visibility, body_storage_key FROM sessions WHERE id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        })?;

    if visibility != "public" {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        )
            .into_response());
    }

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
            (axum::http::header::CONTENT_TYPE, "application/json"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"session.hail.json\"",
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

/// Extract text from first UserMessage in a session
fn extract_first_user_text(session: &opensession_core::Session) -> Option<String> {
    for event in &session.events {
        if matches!(event.event_type, opensession_core::EventType::UserMessage) {
            return extract_text_from_content(&event.content);
        }
    }
    None
}

/// Extract concatenated text from first N UserMessages
fn extract_user_texts(session: &opensession_core::Session, n: usize) -> Option<String> {
    let mut texts = Vec::new();
    for event in &session.events {
        if matches!(event.event_type, opensession_core::EventType::UserMessage) {
            if let Some(text) = extract_text_from_content(&event.content) {
                texts.push(text);
                if texts.len() >= n {
                    break;
                }
            }
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join(" | "))
    }
}

fn extract_text_from_content(content: &opensession_core::Content) -> Option<String> {
    for block in &content.blocks {
        if let opensession_core::ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a char boundary near max_len
        let mut end = max_len.saturating_sub(3);
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
