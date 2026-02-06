use worker::*;

mod routes;
mod storage;

#[event(fetch, respond_with_errors)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    let router = Router::new();

    router
        // Health
        .get_async("/api/health", routes::health::handle)
        // Auth
        .post_async("/api/register", routes::auth::register)
        .post_async("/api/auth/verify", routes::auth::verify)
        // Sessions
        .post_async("/api/sessions", routes::sessions::create)
        .get_async("/api/sessions", routes::sessions::list)
        .get_async("/api/sessions/:id", routes::sessions::get)
        .get_async("/api/sessions/:id/raw", routes::sessions::get_raw)
        // Groups
        .post_async("/api/groups", routes::groups::create)
        .get_async("/api/groups", routes::groups::list)
        .get_async("/api/groups/:id", routes::groups::get)
        .put_async("/api/groups/:id", routes::groups::update)
        .get_async("/api/groups/:id/members", routes::groups::members)
        // Invites
        .post_async("/api/groups/:id/invites", routes::invites::create)
        .post_async("/api/invites/:code/join", routes::invites::join)
        .run(req, env)
        .await
}
