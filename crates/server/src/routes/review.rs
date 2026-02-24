use axum::{
    extract::{Path, State},
    Json,
};
use opensession_api::LocalReviewBundle;

use crate::error::ApiErr;
use crate::AppConfig;

/// GET /api/review/local/:review_id — load a local PR review bundle.
pub async fn get_local_review_bundle(
    State(config): State<AppConfig>,
    Path(review_id): Path<String>,
) -> Result<Json<LocalReviewBundle>, ApiErr> {
    let root = config
        .local_review_root
        .as_ref()
        .ok_or_else(|| ApiErr::not_found("local review API is not enabled on this server"))?;

    validate_review_id(&review_id)?;
    let bundle_path = root.join(&review_id).join("bundle.json");
    let body = tokio::fs::read(&bundle_path)
        .await
        .map_err(|_| ApiErr::not_found("local review bundle not found"))?;

    let parsed: LocalReviewBundle =
        serde_json::from_slice(&body).map_err(|_| ApiErr::internal("invalid review bundle"))?;
    Ok(Json(parsed))
}

fn validate_review_id(review_id: &str) -> Result<(), ApiErr> {
    if review_id.is_empty() {
        return Err(ApiErr::bad_request("review_id is required"));
    }
    let is_valid = review_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
    if !is_valid {
        return Err(ApiErr::bad_request("review_id contains invalid characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_review_id;

    #[test]
    fn review_id_accepts_safe_chars() {
        assert!(validate_review_id("gh-org-repo-pr1-abcdef1").is_ok());
        assert!(validate_review_id("abc.DEF_123").is_ok());
    }

    #[test]
    fn review_id_rejects_empty_or_traversal_tokens() {
        assert!(validate_review_id("").is_err());
        assert!(validate_review_id("../oops").is_err());
        assert!(validate_review_id("bad/name").is_err());
        assert!(validate_review_id("bad%20id").is_err());
    }
}
