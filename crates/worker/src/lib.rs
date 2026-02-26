use worker::*;

mod config;
mod db_helpers;
mod error;
mod routes;
mod storage;

use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::IntoErrResponse;

static D1_SCHEMA_READY: AtomicBool = AtomicBool::new(false);

fn cors_headers(headers: &mut Headers) {
    let _ = headers.set("Access-Control-Allow-Origin", "*");
    let _ = headers.set(
        "Access-Control-Allow-Methods",
        "GET, POST, PUT, DELETE, OPTIONS",
    );
    let _ = headers.set(
        "Access-Control-Allow-Headers",
        "Content-Type, Authorization",
    );
    let _ = headers.set("Access-Control-Max-Age", "86400");
}

fn cors_response() -> Result<Response> {
    let mut headers = Headers::new();
    cors_headers(&mut headers);
    Ok(Response::empty()?.with_headers(headers).with_status(204))
}

fn with_cors(resp: Response) -> Result<Response> {
    let mut headers = Headers::new();
    cors_headers(&mut headers);
    // Merge cors headers into existing response headers
    let existing = resp.headers().clone();
    for (k, v) in existing.entries() {
        let _ = headers.set(&k, &v);
    }
    Ok(resp.with_headers(headers))
}

#[event(fetch, respond_with_errors)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    // Handle CORS preflight
    if req.method() == Method::Options {
        return cors_response();
    }

    if !D1_SCHEMA_READY.load(Ordering::Relaxed) {
        if let Err(err) = storage::ensure_d1_schema(&env).await {
            let service_error =
                opensession_api::ServiceError::Internal(format!("initialize d1 schema: {err}"));
            return service_error.into_err_response();
        }
        D1_SCHEMA_READY.store(true, Ordering::Relaxed);
    }

    let router = Router::new()
        // Health
        .get_async("/api/health", routes::health::handle)
        .get_async("/api/capabilities", routes::capabilities::handle)
        .post_async("/api/parse/preview", routes::parse::preview)
        // Public sessions (read-only)
        .get_async("/api/sessions", routes::sessions::list)
        .get_async("/api/sessions/:id", routes::sessions::get)
        .get_async("/api/sessions/:id/raw", routes::sessions::get_raw)
        // Auth
        .get_async("/api/auth/providers", routes::auth::providers)
        .post_async("/api/auth/register", routes::auth::auth_register)
        .post_async("/api/auth/login", routes::auth::login)
        .post_async("/api/auth/refresh", routes::auth::refresh)
        .post_async("/api/auth/logout", routes::auth::logout)
        .post_async("/api/auth/verify", routes::auth::verify)
        .post_async("/api/auth/api-keys/issue", routes::auth::issue_api_key)
        .get_async("/api/auth/me", routes::auth::me)
        .get_async("/api/auth/oauth/:provider", routes::auth::oauth_redirect)
        .get_async(
            "/api/auth/oauth/:provider/callback",
            routes::auth::oauth_callback,
        )
        // Docs (content negotiation: markdown for AI agents, HTML for browsers)
        .get_async("/docs", routes::docs::handle)
        .get_async("/llms.txt", routes::docs::llms_txt);

    let resp = router.run(req, env).await?;

    with_cors(resp)
}
