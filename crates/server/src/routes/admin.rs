use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use opensession_api::{db, OkResponse, SessionSummary};

use crate::error::ApiErr;
use crate::storage::{session_from_row, sq_execute, sq_query_row, Db};
use crate::AppConfig;

/// DELETE /api/admin/sessions/:id — delete a session (admin key required).
pub async fn delete_session(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<OkResponse>, ApiErr> {
    let provided = headers
        .get("X-OpenSession-Admin-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or("");

    if config.admin_key.trim().is_empty() || provided != config.admin_key.trim() {
        return Err(ApiErr::unauthorized("invalid admin key"));
    }

    let conn = db.conn();
    let _summary: SessionSummary =
        sq_query_row(&conn, db::sessions::get_by_id(&id), session_from_row)
            .map_err(|_| ApiErr::not_found("session not found"))?;

    sq_execute(&conn, db::sessions::delete_links(&id)).map_err(ApiErr::from_db("delete links"))?;
    sq_execute(&conn, db::sessions::delete(&id)).map_err(ApiErr::from_db("delete session"))?;

    let _ = sq_execute(&conn, db::sessions::delete_fts(&id));
    Ok(Json(OkResponse { ok: true }))
}
