use opensession_api::CapabilitiesResponse;
use worker::*;

use crate::config::WorkerConfig;

pub async fn handle(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = WorkerConfig::from_env(&ctx.env);
    Response::from_json(&CapabilitiesResponse {
        auth_enabled: config.auth_enabled(),
        upload_enabled: false,
        ingest_preview_enabled: false,
        gh_share_enabled: false,
    })
}
