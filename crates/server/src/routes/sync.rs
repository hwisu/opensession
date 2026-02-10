use axum::{
    extract::{Query, State},
    Json,
};

use opensession_api_types::{db, SessionSummary, SyncPullQuery, SyncPullResponse};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, Db};

/// `GET /api/sync/pull` — incremental pull of team sessions.
///
/// Returns sessions uploaded after the given cursor (`since`), ordered by `uploaded_at ASC`.
/// The client should store `next_cursor` and pass it as `since` on the next call.
pub async fn pull(
    State(db): State<Db>,
    user: AuthUser,
    Query(q): Query<SyncPullQuery>,
) -> Result<Json<SyncPullResponse>, ApiErr> {
    if !db.is_team_member(&q.team_id, &user.user_id) {
        return Err(ApiErr::forbidden("not a member of this team"));
    }

    let conn = db.conn();
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

    let mut stmt = conn
        .prepare(&sql)
        .map_err(ApiErr::from_db("prepare pull"))?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), session_from_row)
        .map_err(ApiErr::from_db("pull sessions"))?;

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
