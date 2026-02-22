use worker::*;

pub async fn removed_route(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json; charset=utf-8")?;
    Ok(Response::from_json(&serde_json::json!({
        "code": "not_found",
        "message": "legacy route removed; use /src/<provider>/... canonical source routes",
    }))?
    .with_headers(headers)
    .with_status(404))
}
