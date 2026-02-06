use serde::Serialize;
use worker::*;

use crate::storage;

#[derive(Serialize)]
struct SessionMeta {
    id: String,
    title: Option<String>,
    agent_provider: String,
    agent_model: String,
    agent_tool: String,
    event_count: i64,
    duration_seconds: i64,
    created_at: String,
    updated_at: String,
}

/// POST /api/sessions - upload a new session
pub async fn create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // Authenticate
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    // Parse the session body
    let body_bytes = req.bytes().await?;
    let session: opensession_core::Session = serde_json::from_slice(&body_bytes)
        .map_err(|e| Error::from(format!("Invalid session JSON: {e}")))?;

    let session_id = &session.session_id;

    // Store metadata in D1
    let db = storage::get_d1(&ctx.env)?;
    let stmt = db.prepare(
        "INSERT INTO sessions (id, user_id, title, agent_provider, agent_model, agent_tool, event_count, duration_seconds, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    );
    stmt.bind(&[
        session_id.clone().into(),
        user.id.clone().into(),
        session.context.title.clone().unwrap_or_default().into(),
        session.agent.provider.clone().into(),
        session.agent.model.clone().into(),
        session.agent.tool.clone().into(),
        (session.stats.event_count as i64).into(),
        (session.stats.duration_seconds as i64).into(),
        session.context.created_at.to_rfc3339().into(),
        session.context.updated_at.to_rfc3339().into(),
    ])?
    .run()
    .await?;

    // Store full body in R2
    storage::put_session_body(&ctx.env, session_id, &body_bytes).await?;

    Response::from_json(&serde_json::json!({
        "id": session_id,
        "status": "created",
    }))
}

/// GET /api/sessions - list sessions for the authenticated user
pub async fn list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    let db = storage::get_d1(&ctx.env)?;
    let stmt = db.prepare(
        "SELECT id, user_id, title, agent_provider, agent_model, agent_tool, event_count, duration_seconds, created_at, updated_at \
         FROM sessions WHERE user_id = ?1 ORDER BY created_at DESC",
    );
    let results = stmt
        .bind(&[user.id.into()])?
        .all()
        .await?;

    let rows = results.results::<storage::SessionRow>()?;
    let sessions: Vec<SessionMeta> = rows
        .into_iter()
        .map(|r| SessionMeta {
            id: r.id,
            title: r.title,
            agent_provider: r.agent_provider,
            agent_model: r.agent_model,
            agent_tool: r.agent_tool,
            event_count: r.event_count,
            duration_seconds: r.duration_seconds,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect();

    Response::from_json(&sessions)
}

/// GET /api/sessions/:id - get session metadata
pub async fn get(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let db = storage::get_d1(&ctx.env)?;
    let stmt = db.prepare(
        "SELECT id, user_id, title, agent_provider, agent_model, agent_tool, event_count, duration_seconds, created_at, updated_at \
         FROM sessions WHERE id = ?1 AND user_id = ?2",
    );
    let row = stmt
        .bind(&[id.clone().into(), user.id.into()])?
        .first::<storage::SessionRow>(None)
        .await?;

    match row {
        Some(r) => Response::from_json(&SessionMeta {
            id: r.id,
            title: r.title,
            agent_provider: r.agent_provider,
            agent_model: r.agent_model,
            agent_tool: r.agent_tool,
            event_count: r.event_count,
            duration_seconds: r.duration_seconds,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }),
        None => Response::error("Not found", 404),
    }
}

/// GET /api/sessions/:id/raw - get the full session JSON from R2
pub async fn get_raw(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    // Verify ownership via D1
    let db = storage::get_d1(&ctx.env)?;
    let stmt = db.prepare("SELECT id, user_id, title, agent_provider, agent_model, agent_tool, event_count, duration_seconds, created_at, updated_at FROM sessions WHERE id = ?1 AND user_id = ?2");
    let exists = stmt
        .bind(&[id.clone().into(), user.id.into()])?
        .first::<storage::SessionRow>(None)
        .await?;

    if exists.is_none() {
        return Response::error("Not found", 404);
    }

    // Fetch from R2
    match storage::get_session_body(&ctx.env, id).await? {
        Some(bytes) => {
            let mut headers = Headers::new();
            headers.set("Content-Type", "application/json")?;
            Ok(Response::from_bytes(bytes)?.with_headers(headers))
        }
        None => Response::error("Session body not found", 404),
    }
}
