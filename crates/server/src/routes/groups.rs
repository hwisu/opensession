use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    CreateGroupRequest, GroupDetailResponse, GroupResponse, ListGroupsResponse, SessionSummary,
    UpdateGroupRequest,
};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

pub async fn create_group(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<GroupResponse>), Response> {
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must be 1-128 characters"})),
        )
            .into_response());
    }

    let group_id = Uuid::new_v4().to_string();
    let conn = db.conn();

    conn.execute(
        "INSERT INTO groups (id, name, description, is_public, owner_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![&group_id, &name, &req.description, req.is_public, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("create group: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create group"})),
        )
            .into_response()
    })?;

    // Add owner as admin member
    conn.execute(
        "INSERT INTO group_members (group_id, user_id, role) VALUES (?1, ?2, 'admin')",
        rusqlite::params![&group_id, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("add owner as member: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create group"})),
        )
            .into_response()
    })?;

    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM groups WHERE id = ?1",
            [&group_id],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(GroupResponse {
            id: group_id,
            name,
            description: req.description,
            is_public: req.is_public,
            owner_id: user.user_id,
            created_at,
        }),
    ))
}

// ---------------------------------------------------------------------------
// List my groups
// ---------------------------------------------------------------------------

pub async fn list_my_groups(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<ListGroupsResponse>, Response> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT g.id, g.name, g.description, g.is_public, g.owner_id, g.created_at
             FROM groups g
             INNER JOIN group_members gm ON gm.group_id = g.id
             WHERE gm.user_id = ?1
             ORDER BY g.created_at DESC",
        )
        .map_err(internal_error)?;

    let groups: Vec<GroupResponse> = stmt
        .query_map([&user.user_id], |row| {
            Ok(GroupResponse {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                is_public: row.get(3)?,
                owner_id: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListGroupsResponse { groups }))
}

// ---------------------------------------------------------------------------
// Get group detail + sessions
// ---------------------------------------------------------------------------

pub async fn get_group(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<GroupDetailResponse>, Response> {
    let conn = db.conn();

    let group = conn
        .query_row(
            "SELECT id, name, description, is_public, owner_id, created_at FROM groups WHERE id = ?1",
            [&id],
            |row| {
                Ok(GroupResponse {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    is_public: row.get(3)?,
                    owner_id: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "group not found"})),
            )
                .into_response()
        })?;

    if !group.is_public {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "group not found"})),
        )
            .into_response());
    }

    let member_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM group_members WHERE group_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.user_id, u.nickname, s.tool, s.agent_provider, s.agent_model, s.title, s.description, s.tags, s.visibility, s.created_at, s.uploaded_at, s.message_count, s.task_count, s.event_count, s.duration_seconds, u.avatar_url
             FROM sessions s
             LEFT JOIN users u ON u.id = s.user_id
             INNER JOIN session_groups sg ON sg.session_id = s.id
             WHERE sg.group_id = ?1 AND s.visibility = 'public'
             ORDER BY s.uploaded_at DESC
             LIMIT 50",
        )
        .map_err(internal_error)?;

    let sessions: Vec<SessionSummary> = stmt
        .query_map([&id], |row| {
            Ok(SessionSummary {
                id: row.get(0)?,
                user_id: row.get(1)?,
                nickname: row.get(2)?,
                tool: row.get(3)?,
                agent_provider: row.get(4)?,
                agent_model: row.get(5)?,
                title: row.get(6)?,
                description: row.get(7)?,
                tags: row.get(8)?,
                visibility: row.get(9)?,
                created_at: row.get(10)?,
                uploaded_at: row.get(11)?,
                message_count: row.get(12)?,
                task_count: row.get(13)?,
                event_count: row.get(14)?,
                duration_seconds: row.get(15)?,
                avatar_url: row.get(16)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(GroupDetailResponse {
        group,
        member_count,
        sessions,
    }))
}

// ---------------------------------------------------------------------------
// Update group
// ---------------------------------------------------------------------------

pub async fn update_group(
    State(db): State<Db>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<Json<GroupResponse>, Response> {
    let conn = db.conn();

    // Check user is owner or admin
    let role: Option<String> = conn
        .query_row(
            "SELECT gm.role FROM group_members gm
             INNER JOIN groups g ON g.id = gm.group_id
             WHERE gm.group_id = ?1 AND gm.user_id = ?2",
            rusqlite::params![&id, &user.user_id],
            |row| row.get(0),
        )
        .ok();

    let is_owner: bool = conn
        .query_row(
            "SELECT owner_id = ?2 FROM groups WHERE id = ?1",
            rusqlite::params![&id, &user.user_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !is_owner && role.as_deref() != Some("admin") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "must be group owner or admin"})),
        )
            .into_response());
    }

    if let Some(ref name) = req.name {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "name must be 1-128 characters"})),
            )
                .into_response());
        }
        conn.execute(
            "UPDATE groups SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, &id],
        )
        .map_err(internal_error)?;
    }

    if let Some(ref desc) = req.description {
        conn.execute(
            "UPDATE groups SET description = ?1 WHERE id = ?2",
            rusqlite::params![desc, &id],
        )
        .map_err(internal_error)?;
    }

    if let Some(is_public) = req.is_public {
        conn.execute(
            "UPDATE groups SET is_public = ?1 WHERE id = ?2",
            rusqlite::params![is_public, &id],
        )
        .map_err(internal_error)?;
    }

    let group = conn
        .query_row(
            "SELECT id, name, description, is_public, owner_id, created_at FROM groups WHERE id = ?1",
            [&id],
            |row| {
                Ok(GroupResponse {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    is_public: row.get(3)?,
                    owner_id: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "group not found"})),
            )
                .into_response()
        })?;

    Ok(Json(group))
}

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("db error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal server error"})),
    )
        .into_response()
}
