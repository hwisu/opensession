use axum::{
    extract::{Query, State},
    Json,
};

use opensession_api_types::{db, SessionSummary, SyncPullQuery, SyncPullResponse};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, sq_query_map, Db};

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

    let limit = q.limit.unwrap_or(100).clamp(1, 500) as u64;

    // Cursor format: "{uploaded_at}\n{session_id}" (opaque to clients).
    let (cursor_at, cursor_id) = if let Some(ref since) = q.since {
        if let Some((ts, last_id)) = since.split_once('\n') {
            (Some(ts.to_owned()), Some(last_id.to_owned()))
        } else {
            // Legacy cursor (plain timestamp) — best-effort.
            // Use empty string as session_id sentinel so that the keyset
            // condition `(uploaded_at, id) > (ts, "")` skips past that timestamp.
            (Some(since.clone()), Some(String::new()))
        }
    } else {
        (None, None)
    };

    let conn = db.conn();
    let sessions: Vec<SessionSummary> = sq_query_map(
        &conn,
        db::sessions::sync_pull(
            &q.team_id,
            cursor_at.as_deref(),
            cursor_id.as_deref(),
            limit,
        ),
        session_from_row,
    )
    .map_err(ApiErr::from_db("pull sessions"))?;

    let has_more = sessions.len() as u64 == limit;
    let next_cursor = if has_more {
        sessions
            .last()
            .map(|s| format!("{}\n{}", s.uploaded_at, s.id))
    } else {
        None
    };

    Ok(Json(SyncPullResponse {
        sessions,
        next_cursor,
        has_more,
    }))
}
