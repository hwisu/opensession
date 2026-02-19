use anyhow::{ensure, Result};
use uuid::Uuid;

use crate::client::TestContext;

/// Anonymous GET /api/sessions returns a paginated list payload.
pub async fn list_sessions_public(ctx: &TestContext) -> Result<()> {
    let resp = ctx.get("/sessions?page=1&per_page=10").await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: serde_json::Value = resp.json().await?;
    ensure!(body["sessions"].is_array(), "sessions must be an array");
    ensure!(body["page"].is_number(), "page must be present");
    ensure!(body["per_page"].is_number(), "per_page must be present");
    ensure!(body["total"].is_number(), "total must be present");
    Ok(())
}

/// Anonymous GET /api/sessions/{id} for a missing session should return 404.
pub async fn get_session_not_found_public(ctx: &TestContext) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let resp = ctx.get(&format!("/sessions/{id}")).await?;
    ensure!(resp.status() == 404, "expected 404, got {}", resp.status());
    Ok(())
}

/// Anonymous GET /api/sessions/{id}/raw for a missing session should return 404.
pub async fn get_session_raw_not_found_public(ctx: &TestContext) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let resp = ctx.get(&format!("/sessions/{id}/raw")).await?;
    ensure!(resp.status() == 404, "expected 404, got {}", resp.status());
    Ok(())
}
