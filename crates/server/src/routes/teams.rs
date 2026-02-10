use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    db, service, AddMemberRequest, CreateTeamRequest, ListMembersResponse, ListTeamsResponse,
    MemberResponse, SessionSummary, TeamDetailResponse, TeamResponse, UpdateTeamRequest,
};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, team_from_row, Db};

// ---------------------------------------------------------------------------
// Create team (admin only)
// ---------------------------------------------------------------------------

pub async fn create_team(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), ApiErr> {
    if !user.is_admin {
        return Err(ApiErr::forbidden("admin only"));
    }

    let name = service::validate_team_name(&req.name).map_err(ApiErr::from)?;

    let team_id = Uuid::new_v4().to_string();
    let is_public = req.is_public.unwrap_or(false);
    let conn = db.conn();

    conn.execute(
        db::TEAM_INSERT,
        rusqlite::params![&team_id, &name, &req.description, is_public, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("create team: {e}");
        ApiErr::internal("failed to create team")
    })?;

    // Add creator as admin member
    conn.execute(
        "INSERT INTO team_members (team_id, user_id, role) VALUES (?1, ?2, 'admin')",
        rusqlite::params![&team_id, &user.user_id],
    )
    .map_err(|e| {
        tracing::error!("add creator as member: {e}");
        ApiErr::internal("failed to create team")
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
            is_public,
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
) -> Result<Json<ListTeamsResponse>, ApiErr> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(&db::TEAM_LIST_MY)
        .map_err(ApiErr::from_db("prepare teams"))?;

    let teams: Vec<TeamResponse> = stmt
        .query_map([&user.user_id], team_from_row)
        .map_err(ApiErr::from_db("list teams"))?
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
) -> Result<Json<TeamDetailResponse>, ApiErr> {
    let conn = db.conn();

    let team = conn
        .query_row(&db::TEAM_GET, [&id], team_from_row)
        .map_err(|_| ApiErr::not_found("team not found"))?;

    let member_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM team_members WHERE team_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let sessions_sql = format!(
        "SELECT {} \
         FROM sessions s \
         LEFT JOIN users u ON u.id = s.user_id \
         WHERE s.team_id = ?1 \
         ORDER BY s.uploaded_at DESC \
         LIMIT 50",
        db::SESSION_COLUMNS,
    );
    let mut stmt = conn
        .prepare(&sessions_sql)
        .map_err(ApiErr::from_db("prepare team sessions"))?;

    let sessions: Vec<SessionSummary> = stmt
        .query_map([&id], session_from_row)
        .map_err(ApiErr::from_db("list team sessions"))?
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
) -> Result<Json<TeamResponse>, ApiErr> {
    if !user.is_admin {
        return Err(ApiErr::forbidden("admin only"));
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
        return Err(ApiErr::not_found("team not found"));
    }

    if let Some(ref name) = req.name {
        let name = service::validate_team_name(name).map_err(ApiErr::from)?;
        conn.execute(
            "UPDATE teams SET name = ?1 WHERE id = ?2",
            rusqlite::params![&name, &id],
        )
        .map_err(ApiErr::from_db("update team name"))?;
    }

    if let Some(ref desc) = req.description {
        conn.execute(
            "UPDATE teams SET description = ?1 WHERE id = ?2",
            rusqlite::params![desc, &id],
        )
        .map_err(ApiErr::from_db("update team description"))?;
    }

    if let Some(is_public) = req.is_public {
        conn.execute(
            "UPDATE teams SET is_public = ?1 WHERE id = ?2",
            rusqlite::params![is_public, &id],
        )
        .map_err(ApiErr::from_db("update team visibility"))?;
    }

    let team = conn
        .query_row(&db::TEAM_GET, [&id], team_from_row)
        .map_err(|_| ApiErr::not_found("team not found"))?;

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
) -> Result<(StatusCode, Json<MemberResponse>), ApiErr> {
    if !user.is_admin {
        return Err(ApiErr::forbidden("admin only"));
    }

    let conn = db.conn();

    // Look up user by nickname
    let target = conn
        .query_row(
            "SELECT id FROM users WHERE nickname = ?1",
            [&req.nickname],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| ApiErr::not_found("user not found"))?;

    // Check not already a member
    let already: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&team_id, &target],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if already {
        return Err(ApiErr::conflict("already a member"));
    }

    conn.execute(
        "INSERT INTO team_members (team_id, user_id, role) VALUES (?1, ?2, 'member')",
        rusqlite::params![&team_id, &target],
    )
    .map_err(|e| {
        tracing::error!("add member: {e}");
        ApiErr::internal("failed to add member")
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
) -> Result<StatusCode, ApiErr> {
    if !user.is_admin {
        return Err(ApiErr::forbidden("admin only"));
    }

    let conn = db.conn();
    let affected = conn
        .execute(
            "DELETE FROM team_members WHERE team_id = ?1 AND user_id = ?2",
            rusqlite::params![&team_id, &user_id],
        )
        .map_err(|e| {
            tracing::error!("remove member: {e}");
            ApiErr::internal("failed to remove member")
        })?;

    if affected == 0 {
        return Err(ApiErr::not_found("member not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// List members
// ---------------------------------------------------------------------------

pub async fn list_members(
    State(db): State<Db>,
    Path(team_id): Path<String>,
) -> Result<Json<ListMembersResponse>, ApiErr> {
    let conn = db.conn();

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM teams WHERE id = ?1",
            [&team_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err(ApiErr::not_found("team not found"));
    }

    let mut stmt = conn
        .prepare(&db::MEMBER_LIST)
        .map_err(ApiErr::from_db("prepare members"))?;

    let members: Vec<MemberResponse> = stmt
        .query_map([&team_id], |row| {
            Ok(MemberResponse {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                role: row.get(2)?,
                joined_at: row.get(3)?,
            })
        })
        .map_err(ApiErr::from_db("list members"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListMembersResponse { members }))
}
