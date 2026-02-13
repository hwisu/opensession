use anyhow::{ensure, Result};

use crate::client::TestContext;

pub async fn health_check(ctx: &TestContext) -> Result<()> {
    let resp = ctx
        .api
        .reqwest_client()
        .get(ctx.url("/health"))
        .send()
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: serde_json::Value = resp.json().await?;
    ensure!(body["status"] == "ok", "expected status=ok");
    ensure!(body["version"].is_string(), "expected version string");
    Ok(())
}
