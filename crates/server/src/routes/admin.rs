use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use opensession_api::OkResponse;

use crate::AppConfig;
use crate::error::ApiErr;
use crate::storage::Db;

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

    let deleted = db
        .delete_session(&id)
        .await
        .map_err(ApiErr::from_db("delete session"))?;
    if !deleted {
        return Err(ApiErr::not_found("session not found"));
    }
    Ok(Json(OkResponse { ok: true }))
}
