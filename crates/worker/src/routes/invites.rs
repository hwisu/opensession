use serde::{Deserialize, Serialize};
use worker::*;

use crate::storage;

#[derive(Deserialize)]
struct CreateInviteRequest {
    #[serde(default)]
    expires_in_hours: Option<i64>,
}

#[derive(Serialize)]
struct InviteResponse {
    id: String,
    group_id: String,
    code: String,
    expires_at: Option<String>,
}

/// POST /api/groups/:id/invites
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

    let group_id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let db = storage::get_d1(&ctx.env)?;

    // Verify user is owner or admin of the group
    let member = db
        .prepare("SELECT user_id, '' as username, role, joined_at FROM group_members WHERE group_id = ?1 AND user_id = ?2")
        .bind(&[group_id.clone().into(), user.id.clone().into()])?
        .first::<storage::MemberRow>(None)
        .await?;

    match &member {
        Some(m) if m.role == "owner" || m.role == "admin" => {}
        _ => return Response::error("Not authorized to create invites", 403),
    }

    let body: CreateInviteRequest = req.json().await.unwrap_or(CreateInviteRequest {
        expires_in_hours: None,
    });

    let invite_id = uuid::Uuid::new_v4().to_string();
    // Generate a short invite code
    let code = uuid::Uuid::new_v4().to_string()[..8].to_string();

    let expires_at = body.expires_in_hours.map(|h| {
        format!("datetime('now', '+{h} hours')")
    });

    if let Some(_hours) = body.expires_in_hours {
        db.prepare(
            "INSERT INTO invites (id, group_id, code, created_by, expires_at) VALUES (?1, ?2, ?3, ?4, datetime('now', '+' || ?5 || ' hours'))",
        )
        .bind(&[
            invite_id.clone().into(),
            group_id.clone().into(),
            code.clone().into(),
            user.id.into(),
            _hours.into(),
        ])?
        .run()
        .await?;
    } else {
        db.prepare(
            "INSERT INTO invites (id, group_id, code, created_by, expires_at) VALUES (?1, ?2, ?3, ?4, NULL)",
        )
        .bind(&[
            invite_id.clone().into(),
            group_id.clone().into(),
            code.clone().into(),
            user.id.into(),
        ])?
        .run()
        .await?;
    }

    Response::from_json(&InviteResponse {
        id: invite_id,
        group_id: group_id.clone(),
        code,
        expires_at: expires_at.map(|_| "set".to_string()),
    })
}

/// POST /api/invites/:code/join
pub async fn join(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth = req
        .headers()
        .get("Authorization")?
        .ok_or_else(|| Error::from("Missing Authorization header"))?;
    let api_key = storage::extract_bearer(&auth)
        .ok_or_else(|| Error::from("Invalid Authorization header"))?;
    let user = storage::authenticate(&ctx.env, api_key)
        .await?
        .ok_or_else(|| Error::from("Unauthorized"))?;

    let code = ctx
        .param("code")
        .ok_or_else(|| Error::from("Missing code"))?;

    let db = storage::get_d1(&ctx.env)?;

    // Find the invite
    let invite = db
        .prepare("SELECT id, group_id, code, created_by, expires_at FROM invites WHERE code = ?1")
        .bind(&[code.clone().into()])?
        .first::<storage::InviteRow>(None)
        .await?;

    let invite = match invite {
        Some(inv) => inv,
        None => return Response::error("Invalid invite code", 404),
    };

    // Check if already a member
    let existing = db
        .prepare("SELECT user_id, '' as username, role, joined_at FROM group_members WHERE group_id = ?1 AND user_id = ?2")
        .bind(&[invite.group_id.clone().into(), user.id.clone().into()])?
        .first::<storage::MemberRow>(None)
        .await?;

    if existing.is_some() {
        return Response::error("Already a member", 409);
    }

    // Add the user as a member
    db.prepare(
        "INSERT INTO group_members (group_id, user_id, role, joined_at) VALUES (?1, ?2, 'member', datetime('now'))",
    )
    .bind(&[invite.group_id.clone().into(), user.id.into()])?
    .run()
    .await?;

    Response::from_json(&serde_json::json!({
        "group_id": invite.group_id,
        "status": "joined",
    }))
}
