use worker::*;

use opensession_api::HealthResponse;

pub async fn handle(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
