use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use opensession_api_types::{db, SessionSummary, SyncPullQuery, SyncPullResponse};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

/// `GET /api/sync/pull` — incremental pull of team sessions.
///
/// Returns sessions uploaded after the given cursor (`since`), ordered by `uploaded_at ASC`.
/// The client should store `next_cursor` and pass it as `since` on the next call.
pub async fn pull(
    State(db): State<Db>,
    user: AuthUser,
    Query(q): Query<SyncPullQuery>,
) -> Result<Json<SyncPullResponse>, Response> {
    let conn = db.conn();

    // Verify user is a member of the team
    let is_member: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&q.team_id, &user.user_id],
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

    let limit = q.limit.unwrap_or(100).clamp(1, 500) as i64;

    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref since) = q.since {
            (
                format!(
                    "SELECT {} \
                 FROM sessions s LEFT JOIN users u ON u.id = s.user_id \
                 WHERE s.team_id = ?1 AND s.uploaded_at > ?2 \
                 ORDER BY s.uploaded_at ASC \
                 LIMIT ?3",
                    db::SESSION_COLUMNS
                ),
                vec![
                    Box::new(q.team_id.clone()),
                    Box::new(since.clone()),
                    Box::new(limit),
                ],
            )
        } else {
            (
                format!(
                    "SELECT {} \
                 FROM sessions s LEFT JOIN users u ON u.id = s.user_id \
                 WHERE s.team_id = ?1 \
                 ORDER BY s.uploaded_at ASC \
                 LIMIT ?2",
                    db::SESSION_COLUMNS
                ),
                vec![Box::new(q.team_id.clone()), Box::new(limit)],
            )
        };

    let mut stmt = conn.prepare(&sql).map_err(internal_error)?;
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

    let has_more = sessions.len() as i64 == limit;
    let next_cursor = if has_more {
        sessions.last().map(|s| s.uploaded_at.clone())
    } else {
        None
    };

    Ok(Json(SyncPullResponse {
        sessions,
        next_cursor,
        has_more,
    }))
}

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("db error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal server error"})),
    )
        .into_response()
}
