use axum::{http::StatusCode, Json};
use serde_json::json;

pub async fn removed_route() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "code": "not_found",
            "message": "legacy route removed; use /src/<provider>/... canonical source routes",
        })),
    )
}
