use serde::{Deserialize, Serialize};
use worker::*;

use crate::storage;

#[derive(Deserialize)]
struct CreateGroupRequest {
    name: String,
    description: Option<String>,
}

#[derive(Serialize)]
struct GroupResponse {
    id: String,
    name: String,
    description: Option<String>,
    owner_id: String,
    created_at: String,
}

#[derive(Serialize)]
struct MemberResponse {
    user_id: String,
    username: String,
    role: String,
    joined_at: String,
}

/// POST /api/groups
pub async fn create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    let body: CreateGroupRequest = req.json().await?;

    if body.name.is_empty() {
        return Response::error("name is required", 400);
    }

    let group_id = uuid::Uuid::new_v4().to_string();

    let db = storage::get_d1(&ctx.env)?;

    // Create the group
    db.prepare(
        "INSERT INTO groups (id, name, description, owner_id, created_at) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
    )
    .bind(&[
        group_id.clone().into(),
        body.name.clone().into(),
        body.description.clone().unwrap_or_default().into(),
        user.id.clone().into(),
    ])?
    .run()
    .await?;

    // Add the owner as a member with 'owner' role
    db.prepare(
        "INSERT INTO group_members (group_id, user_id, role, joined_at) VALUES (?1, ?2, 'owner', datetime('now'))",
    )
    .bind(&[group_id.clone().into(), user.id.clone().into()])?
    .run()
    .await?;

    Response::from_json(&GroupResponse {
        id: group_id,
        name: body.name,
        description: body.description,
        owner_id: user.id,
        created_at: String::new(), // filled by DB
    })
}

/// GET /api/groups
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
    let results = db
        .prepare(
            "SELECT g.id, g.name, g.description, g.owner_id, g.created_at \
             FROM groups g \
             INNER JOIN group_members gm ON g.id = gm.group_id \
             WHERE gm.user_id = ?1 \
             ORDER BY g.created_at DESC",
        )
        .bind(&[user.id.into()])?
        .all()
        .await?;

    let rows = results.results::<storage::GroupRow>()?;
    let groups: Vec<GroupResponse> = rows
        .into_iter()
        .map(|r| GroupResponse {
            id: r.id,
            name: r.name,
            description: r.description,
            owner_id: r.owner_id,
            created_at: r.created_at,
        })
        .collect();

    Response::from_json(&groups)
}

/// GET /api/groups/:id
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

    // Verify membership
    let member = db
        .prepare("SELECT user_id, '' as username, role, joined_at FROM group_members WHERE group_id = ?1 AND user_id = ?2")
        .bind(&[id.clone().into(), user.id.into()])?
        .first::<storage::MemberRow>(None)
        .await?;

    if member.is_none() {
        return Response::error("Not found", 404);
    }

    let row = db
        .prepare("SELECT id, name, description, owner_id, created_at FROM groups WHERE id = ?1")
        .bind(&[id.clone().into()])?
        .first::<storage::GroupRow>(None)
        .await?;

    match row {
        Some(r) => Response::from_json(&GroupResponse {
            id: r.id,
            name: r.name,
            description: r.description,
            owner_id: r.owner_id,
            created_at: r.created_at,
        }),
        None => Response::error("Not found", 404),
    }
}

/// PUT /api/groups/:id
pub async fn update(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    // Only the owner can update
    let group = db
        .prepare("SELECT id, name, description, owner_id, created_at FROM groups WHERE id = ?1 AND owner_id = ?2")
        .bind(&[id.clone().into(), user.id.into()])?
        .first::<storage::GroupRow>(None)
        .await?;

    if group.is_none() {
        return Response::error("Not found or not authorized", 404);
    }

    let body: CreateGroupRequest = req.json().await?;

    db.prepare("UPDATE groups SET name = ?1, description = ?2 WHERE id = ?3")
        .bind(&[
            body.name.clone().into(),
            body.description.clone().unwrap_or_default().into(),
            id.clone().into(),
        ])?
        .run()
        .await?;

    Response::from_json(&GroupResponse {
        id: id.clone(),
        name: body.name,
        description: body.description,
        owner_id: group.unwrap().owner_id,
        created_at: String::new(),
    })
}

/// GET /api/groups/:id/members
pub async fn members(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

    // Verify membership
    let member = db
        .prepare("SELECT user_id, '' as username, role, joined_at FROM group_members WHERE group_id = ?1 AND user_id = ?2")
        .bind(&[id.clone().into(), user.id.clone().into()])?
        .first::<storage::MemberRow>(None)
        .await?;

    if member.is_none() {
        return Response::error("Not found", 404);
    }

    let results = db
        .prepare(
            "SELECT gm.user_id, u.username, gm.role, gm.joined_at \
             FROM group_members gm \
             INNER JOIN users u ON gm.user_id = u.id \
             WHERE gm.group_id = ?1",
        )
        .bind(&[id.clone().into()])?
        .all()
        .await?;

    let rows = results.results::<storage::MemberRow>()?;
    let members: Vec<MemberResponse> = rows
        .into_iter()
        .map(|r| MemberResponse {
            user_id: r.user_id,
            username: r.username,
            role: r.role,
            joined_at: r.joined_at,
        })
        .collect();

    Response::from_json(&members)
}
