use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    AddMemberRequest, CreateTeamRequest, ListMembersResponse, ListTeamsResponse, MemberResponse,
    SessionSummary, TeamDetailResponse, TeamResponse, UpdateTeamRequest,
};

use crate::routes::auth::AuthUser;
use crate::storage::Db;

// ---------------------------------------------------------------------------
// Create team (admin only)
// ---------------------------------------------------------------------------

pub async fn create_team(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), Response> {
    if !user.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }

    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must be 1-128 characters"})),
        )
            .into_response());
    }

    let team_id = Uuid::new_v4().to_string();
    let conn = db.conn();

    conn.execute(
        "INSERT INTO teams (id, name, description, created_by) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![&team_id, &name, &req.description, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("create team: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create team"})),
        )
            .into_response()
    })?;

    // Add creator as admin member
    conn.execute(
        "INSERT INTO team_members (team_id, user_id, role) VALUES (?1, ?2, 'admin')",
        rusqlite::params![&team_id, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("add creator as member: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to create team"})),
        )
            .into_response()
    })?;

    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM teams WHERE id = ?1",
            [&team_id],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(TeamResponse {
            id: team_id,
            name,
            description: req.description,
            created_by: user.user_id,
            created_at,
        }),
    ))
}

// ---------------------------------------------------------------------------
// List my teams
// ---------------------------------------------------------------------------

pub async fn list_my_teams(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<ListTeamsResponse>, Response> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.name, t.description, t.created_by, t.created_at
             FROM teams t
             INNER JOIN team_members tm ON tm.team_id = t.id
             WHERE tm.user_id = ?1
             ORDER BY t.created_at DESC",
        )
        .map_err(internal_error)?;

    let teams: Vec<TeamResponse> = stmt
        .query_map([&user.user_id], |row| {
            Ok(TeamResponse {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_by: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListTeamsResponse { teams }))
}

// ---------------------------------------------------------------------------
// Get team detail + sessions
// ---------------------------------------------------------------------------

pub async fn get_team(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<TeamDetailResponse>, Response> {
    let conn = db.conn();

    let team = conn
        .query_row(
            "SELECT id, name, description, created_by, created_at FROM teams WHERE id = ?1",
            [&id],
            |row| {
                Ok(TeamResponse {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_by: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "team not found"})),
            )
                .into_response()
        })?;

    let member_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM team_members WHERE team_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.user_id, u.nickname, s.team_id, s.tool, s.agent_provider, s.agent_model, s.title, s.description, s.tags, s.created_at, s.uploaded_at, s.message_count, s.task_count, s.event_count, s.duration_seconds
             FROM sessions s
             LEFT JOIN users u ON u.id = s.user_id
             WHERE s.team_id = ?1
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
                team_id: row.get(3)?,
                tool: row.get(4)?,
                agent_provider: row.get(5)?,
                agent_model: row.get(6)?,
                title: row.get(7)?,
                description: row.get(8)?,
                tags: row.get(9)?,
                created_at: row.get(10)?,
                uploaded_at: row.get(11)?,
                message_count: row.get(12)?,
                task_count: row.get(13)?,
                event_count: row.get(14)?,
                duration_seconds: row.get(15)?,
            })
        })
        .map_err(internal_error)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(TeamDetailResponse {
        team,
        member_count,
        sessions,
    }))
}

// ---------------------------------------------------------------------------
// Update team (admin only)
// ---------------------------------------------------------------------------

pub async fn update_team(
    State(db): State<Db>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateTeamRequest>,
) -> Result<Json<TeamResponse>, Response> {
    if !user.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }

    let conn = db.conn();

    // Verify team exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM teams WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "team not found"})),
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
            "UPDATE teams SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, &id],
        )
        .map_err(internal_error)?;
    }

    if let Some(ref desc) = req.description {
        conn.execute(
            "UPDATE teams SET description = ?1 WHERE id = ?2",
            rusqlite::params![desc, &id],
        )
        .map_err(internal_error)?;
    }

    let team = conn
        .query_row(
            "SELECT id, name, description, created_by, created_at FROM teams WHERE id = ?1",
            [&id],
            |row| {
                Ok(TeamResponse {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_by: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "team not found"})),
            )
                .into_response()
        })?;

    Ok(Json(team))
}

// ---------------------------------------------------------------------------
// Add member to team (admin only, by nickname)
// ---------------------------------------------------------------------------

pub async fn add_member(
    State(db): State<Db>,
    user: AuthUser,
    Path(team_id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> Result<(StatusCode, Json<MemberResponse>), Response> {
    if !user.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }

    let conn = db.conn();

    // Look up user by nickname
    let target = conn
        .query_row(
            "SELECT id FROM users WHERE nickname = ?1",
            [&req.nickname],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "user not found"})),
            )
                .into_response()
        })?;

    // Check not already a member
    let already: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&team_id, &target],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if already {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "already a member"})),
        )
            .into_response());
    }

    conn.execute(
        "INSERT INTO team_members (team_id, user_id, role) VALUES (?1, ?2, 'member')",
        rusqlite::params![&team_id, &target],
    )
    .map_err(|e| {
        tracing::error!("add member: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to add member"})),
        )
            .into_response()
    })?;

    let joined_at: String = conn
        .query_row(
            "SELECT joined_at FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&team_id, &target],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(MemberResponse {
            user_id: target,
            nickname: req.nickname,
            role: "member".to_string(),
            joined_at,
        }),
    ))
}

// ---------------------------------------------------------------------------
// Remove member from team (admin only)
// ---------------------------------------------------------------------------

pub async fn remove_member(
    State(db): State<Db>,
    user: AuthUser,
    Path((team_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, Response> {
    if !user.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "admin only"})),
        )
            .into_response());
    }

    let conn = db.conn();
    let affected = conn
        .execute(
            "DELETE FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&team_id, &user_id],
        )
        .map_err(|e| {
            tracing::error!("remove member: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to remove member"})),
            )
                .into_response()
        })?;

    if affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "member not found"})),
        )
            .into_response());
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// List members
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(db): State<Db>,
    Path(team_id): Path<String>,
) -> Result<Json<ListMembersResponse>, Response> {
    let conn = db.conn();

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM teams WHERE id = ?1",
            [&team_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "team not found"})),
        )
            .into_response());
    }

    let mut stmt = conn
        .prepare(
            "SELECT tm.user_id, u.nickname, tm.role, tm.joined_at
             FROM team_members tm
             INNER JOIN users u ON u.id = tm.user_id
             WHERE tm.team_id = ?1
             ORDER BY tm.joined_at ASC",
        )
        .map_err(internal_error)?;

    let members: Vec<MemberResponse> = stmt
        .query_map([&team_id], |row| {
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

fn internal_error(e: impl std::fmt::Display) -> Response {
    tracing::error!("db error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal server error"})),
    )
        .into_response()
}
