use worker::*;

mod crypto;
mod db_helpers;
mod error;
mod routes;
mod storage;

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

    let router = Router::new();

    let resp = router
        // Health
        .get_async("/api/health", routes::health::handle)
        // Auth (legacy)
        .post_async("/api/register", routes::auth::register)
        .post_async("/api/auth/verify", routes::auth::verify)
        .get_async("/api/auth/me", routes::auth::me)
        .post_async("/api/auth/regenerate-key", routes::auth::regenerate_key)
        // Auth (email/password + OAuth)
        .post_async("/api/auth/register", routes::auth::auth_register)
        .post_async("/api/auth/login", routes::auth::login)
        .post_async("/api/auth/refresh", routes::auth::refresh)
        .post_async("/api/auth/logout", routes::auth::logout)
        .put_async("/api/auth/password", routes::auth::change_password)
        // Generic OAuth (any provider)
        .get_async("/api/auth/providers", routes::auth::auth_providers)
        .get_async("/api/auth/oauth/:provider", routes::auth::oauth_redirect)
        .get_async(
            "/api/auth/oauth/:provider/callback",
            routes::auth::oauth_callback,
        )
        .post_async("/api/auth/oauth/:provider/link", routes::auth::oauth_link)
        // Sessions
        .post_async("/api/sessions", routes::sessions::create)
        .get_async("/api/sessions", routes::sessions::list)
        .get_async("/api/sessions/:id", routes::sessions::get)
        .delete_async("/api/sessions/:id", routes::sessions::delete)
        .get_async("/api/sessions/:id/raw", routes::sessions::get_raw)
        // Teams
        .post_async("/api/teams", routes::teams::create)
        .get_async("/api/teams", routes::teams::list)
        .get_async("/api/teams/:id/stats", routes::teams::stats)
        .get_async("/api/teams/:id", routes::teams::get)
        .put_async("/api/teams/:id", routes::teams::update)
        // Team members
        .get_async("/api/teams/:id/members", routes::teams::list_members)
        .post_async("/api/teams/:id/members", routes::teams::add_member)
        .delete_async(
            "/api/teams/:team_id/members/:user_id",
            routes::teams::remove_member,
        )
        // Team invitations
        .post_async("/api/teams/:id/invite", routes::teams::invite_member)
        .get_async("/api/invitations", routes::teams::list_invitations)
        .post_async(
            "/api/invitations/:id/accept",
            routes::teams::accept_invitation,
        )
        .post_async(
            "/api/invitations/:id/decline",
            routes::teams::decline_invitation,
        )
        // Sync
        .get_async("/api/sync/pull", routes::sync::pull)
        // Docs (content negotiation: markdown for AI agents, HTML for browsers)
        .get_async("/docs", routes::docs::handle)
        .get_async("/llms.txt", routes::docs::llms_txt)
        .run(req, env)
        .await?;

    with_cors(resp)
}
