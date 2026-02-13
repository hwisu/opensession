use worker::*;

use opensession_api::{db, ServiceError, SessionSummary, SyncPullResponse};

use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

/// `GET /api/sync/pull` — incremental pull of team sessions.
///
/// Returns sessions uploaded after the given cursor (`since`), ordered by `uploaded_at ASC`.
/// The client should store `next_cursor` and pass it as `since` on the next call.
pub async fn pull(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let url = req.url()?;
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();

    let team_id = match pairs.iter().find(|(k, _)| k == "team_id") {
        Some((_, v)) => v.clone(),
        None => {
            return ServiceError::BadRequest("team_id is required".into()).into_err_response();
        }
    };
    let since = pairs
        .iter()
        .find(|(k, _)| k == "since")
        .map(|(_, v)| v.clone());
    let limit = pairs
        .iter()
        .find(|(k, _)| k == "limit")
        .and_then(|(_, v)| v.parse::<u32>().ok())
        .unwrap_or(100)
        .clamp(1, 500) as i64;

    // Verify team membership
    let d1 = storage::get_d1(&ctx.env)?;
    let (sql, values) = db::teams::member_exists(&team_id, &user.id);
    let member_check = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?;

    if member_check.map(|r| r.count).unwrap_or(0) == 0 {
        return ServiceError::Forbidden("not a member of this team".into()).into_err_response();
    }

    // Cursor format: "{uploaded_at}\n{session_id}" (opaque to clients).
    let (cursor_at, cursor_id) = if let Some(ref cursor) = since {
        if let Some((ts, last_id)) = cursor.split_once('\n') {
            (Some(ts), Some(last_id))
        } else {
            // Legacy cursor (plain timestamp) — best-effort.
            (Some(cursor.as_str()), None)
        }
    } else {
        (None, None)
    };

    let (sql, values) = db::sessions::sync_pull(&team_id, cursor_at, cursor_id, limit as u64);
    let rows_result = d1.prepare(&sql).bind(&values_to_js(&values))?.all().await?;

    let sessions: Vec<SessionSummary> = rows_result
        .results::<storage::SessionRow>()?
        .into_iter()
        .map(SessionSummary::from)
        .collect();

    let has_more = sessions.len() as i64 == limit;
    let next_cursor = if has_more {
        sessions
            .last()
            .map(|s| format!("{}\n{}", s.uploaded_at, s.id))
    } else {
        None
    };

    Response::from_json(&SyncPullResponse {
        sessions,
        next_cursor,
        has_more,
    })
}
