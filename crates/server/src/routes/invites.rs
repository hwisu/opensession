use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    CreateInviteRequest, InviteResponse, JoinResponse, ListMembersResponse, MemberResponse,
};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

pub async fn create_invite(
    State(db): State<Db>,
    user: AuthUser,
    Path(group_id): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> Result<(StatusCode, Json<InviteResponse>), Response> {
    let conn = db.conn();

    // Check user is owner or admin
    let role: Option<String> = conn
        .query_row(
            "SELECT role FROM group_members WHERE group_id = ?1 AND user_id = ?2",
            rusqlite::params![&group_id, &user.user_id],
            |row| row.get(0),
        )
        .ok();

    let is_owner: bool = conn
        .query_row(
            "SELECT owner_id = ?2 FROM groups WHERE id = ?1",
            rusqlite::params![&group_id, &user.user_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !is_owner && role.as_deref() != Some("admin") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "must be group owner or admin to create invites"})),
        )
            .into_response());
    }

    let invite_id = Uuid::new_v4().to_string();
    let code = generate_invite_code();

    conn.execute(
        "INSERT INTO invites (id, group_id, code, created_by, max_uses, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![&invite_id, &group_id, &code, &user.user_id, req.max_uses, req.expires_at],
    )
    .map_err(|e| {
        tracing::error!("create invite: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create invite"})),
        )
            .into_response()
    })?;

    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM invites WHERE id = ?1",
            [&invite_id],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(InviteResponse {
            id: invite_id,
            group_id,
            code,
            max_uses: req.max_uses,
            used_count: 0,
            expires_at: req.expires_at,
            created_at,
        }),
    ))
}

// ---------------------------------------------------------------------------
// Join via invite code
// ---------------------------------------------------------------------------

pub async fn join_via_invite(
    State(db): State<Db>,
    user: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<JoinResponse>, Response> {
    let conn = db.conn();

    // Look up invite
    let invite = conn
        .query_row(
            "SELECT id, group_id, max_uses, used_count, expires_at FROM invites WHERE code = ?1",
            [&code],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "invalid invite code"})),
            )
                .into_response()
        })?;

    let (invite_id, group_id, max_uses, used_count, expires_at) = invite;

    // Check if expired
    if let Some(ref exp) = expires_at {
        if let Ok(exp_time) = chrono::NaiveDateTime::parse_from_str(exp, "%Y-%m-%d %H:%M:%S") {
            let now = chrono::Utc::now().naive_utc();
            if now > exp_time {
                return Err((
                    StatusCode::GONE,
                    Json(serde_json::json!({"error": "invite has expired"})),
                )
                    .into_response());
            }
        }
    }

    // Check max uses
    if let Some(max) = max_uses {
        if used_count >= max {
            return Err((
                StatusCode::GONE,
                Json(serde_json::json!({"error": "invite has reached maximum uses"})),
            )
                .into_response());
        }
    }

    // Check if already a member
    let already_member: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM group_members WHERE group_id = ?1 AND user_id = ?2",
            rusqlite::params![&group_id, &user.user_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if already_member {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "already a member of this group"})),
        )
            .into_response());
    }

    // Add member
    conn.execute(
        "INSERT INTO group_members (group_id, user_id, role) VALUES (?1, ?2, 'member')",
        rusqlite::params![&group_id, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("join group: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to join group"})),
        )
            .into_response()
    })?;

    // Increment used_count
    let _ = conn.execute(
        "UPDATE invites SET used_count = used_count + 1 WHERE id = ?1",
        [&invite_id],
    );

    let group_name: String = conn
        .query_row(
            "SELECT name FROM groups WHERE id = ?1",
            [&group_id],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok(Json(JoinResponse {
        group_id,
        group_name,
    }))
}

// ---------------------------------------------------------------------------
// List members
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(db): State<Db>,
    Path(group_id): Path<String>,
) -> Result<Json<ListMembersResponse>, Response> {
    let conn = db.conn();

    // Verify group exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM groups WHERE id = ?1",
            [&group_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "group not found"})),
        )
            .into_response());
    }

    let mut stmt = conn
        .prepare(
            "SELECT gm.user_id, u.nickname, gm.role, gm.joined_at
             FROM group_members gm
             INNER JOIN users u ON u.id = gm.user_id
             WHERE gm.group_id = ?1
             ORDER BY gm.joined_at ASC",
        )
        .map_err(internal_error)?;

    let members: Vec<MemberResponse> = stmt
        .query_map([&group_id], |row| {
            Ok(MemberResponse {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                role: row.get(2)?,
                joined_at: row.get(3)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListMembersResponse { members }))
}

fn generate_invite_code() -> String {
    // 8-char alphanumeric code
    let id = Uuid::new_v4().simple().to_string();
    id[..8].to_string()
}

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("db error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal server error"})),
    )
        .into_response()
}
