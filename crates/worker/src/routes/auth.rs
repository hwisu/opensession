use serde::{Deserialize, Serialize};
use worker::*;

use crate::storage;

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct RegisterResponse {
    id: String,
    username: String,
    api_key: String,
}

#[derive(Deserialize)]
struct VerifyRequest {
    api_key: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    valid: bool,
    user_id: Option<String>,
    username: Option<String>,
}

/// POST /api/register
pub async fn register(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: RegisterRequest = req.json().await?;

    if body.username.is_empty() || body.password.is_empty() {
        return Response::error("username and password are required", 400);
    }

    let user_id = uuid::Uuid::new_v4().to_string();
    let api_key = uuid::Uuid::new_v4().to_string();

    let db = storage::get_d1(&ctx.env)?;
    let stmt = db.prepare(
        "INSERT INTO users (id, username, password_hash, api_key_hash, created_at) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
    );
    // In production, hash the password and API key properly.
    stmt.bind(&[
        user_id.clone().into(),
        body.username.clone().into(),
        body.password.into(), // placeholder: should be hashed
        api_key.clone().into(),
    ])?
    .run()
    .await?;

    Response::from_json(&RegisterResponse {
        id: user_id,
        username: body.username,
        api_key,
    })
}

/// POST /api/auth/verify
pub async fn verify(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: VerifyRequest = req.json().await?;

    match storage::authenticate(&ctx.env, &body.api_key).await? {
        Some(user) => Response::from_json(&VerifyResponse {
            valid: true,
            user_id: Some(user.id),
            username: Some(user.username),
        }),
        None => Response::from_json(&VerifyResponse {
            valid: false,
            user_id: None,
            username: None,
        }),
    }
}
