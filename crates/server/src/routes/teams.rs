use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use opensession_api_types::{
    db, service, AcceptInvitationResponse, AddMemberRequest, CreateTeamRequest, InvitationResponse,
    InviteRequest, ListInvitationsResponse, ListMembersResponse, ListTeamsResponse, MemberResponse,
    OkResponse, SessionSummary, TeamDetailResponse, TeamResponse, UpdateTeamRequest,
};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, team_from_row, Db};

// ---------------------------------------------------------------------------
// Create team
// ---------------------------------------------------------------------------

/// POST /api/teams — create a new team. Creator is added as team admin.
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
        db::MEMBER_INSERT,
        rusqlite::params![&team_id, &user.user_id, "admin"],
    )
    .map_err(|e| {
        tracing::error!("add creator as member: {e}");
        ApiErr::internal("failed to create team")
    })?;

    let created_at: String = conn
        .query_row(db::TEAM_CREATED_AT, [&team_id], |row| row.get(0))
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

/// GET /api/teams — list teams the authenticated user belongs to.
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

/// GET /api/teams/:id — get team detail with member count and recent sessions.
pub async fn get_team(
    State(db): State<Db>,
    Path(id): Path<String>,
) -> Result<Json<TeamDetailResponse>, ApiErr> {
    let conn = db.conn();

    let team = conn
        .query_row(&db::TEAM_GET, [&id], team_from_row)
        .map_err(|_| ApiErr::not_found("team not found"))?;

    let member_count: i64 = conn
        .query_row(db::TEAM_MEMBER_COUNT, [&id], |row| row.get(0))
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

/// PUT /api/teams/:id — update team name, description, or visibility (admin only).
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
        .query_row(db::TEAM_EXISTS, [&id], |row| {
            row.get::<_, i64>(0).map(|c| c > 0)
        })
        .unwrap_or(false);

    if !exists {
        return Err(ApiErr::not_found("team not found"));
    }

    if let Some(ref name) = req.name {
        let name = service::validate_team_name(name).map_err(ApiErr::from)?;
        conn.execute(db::TEAM_UPDATE_NAME, rusqlite::params![&name, &id])
            .map_err(ApiErr::from_db("update team name"))?;
    }

    if let Some(ref desc) = req.description {
        conn.execute(db::TEAM_UPDATE_DESCRIPTION, rusqlite::params![desc, &id])
            .map_err(ApiErr::from_db("update team description"))?;
    }

    if let Some(is_public) = req.is_public {
        conn.execute(
            db::TEAM_UPDATE_VISIBILITY,
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

/// POST /api/teams/:id/members — add a member by nickname (admin only).
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
        .query_row(db::USER_BY_NICKNAME, [&req.nickname], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| ApiErr::not_found("user not found"))?;

    // Check not already a member
    let already: bool = conn
        .query_row(
            db::TEAM_MEMBER_EXISTS,
            rusqlite::params![&team_id, &target],
            |row| row.get::<_, i64>(0).map(|c| c > 0),
        )
        .unwrap_or(false);

    if already {
        return Err(ApiErr::conflict("already a member"));
    }

    conn.execute(
        db::MEMBER_INSERT,
        rusqlite::params![&team_id, &target, "member"],
    )
    .map_err(|e| {
        tracing::error!("add member: {e}");
        ApiErr::internal("failed to add member")
    })?;

    let joined_at: String = conn
        .query_row(
            db::MEMBER_JOINED_AT,
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

/// DELETE /api/teams/:id/members/:user_id — remove a member (admin only).
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
        .execute(db::MEMBER_DELETE, rusqlite::params![&team_id, &user_id])
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

/// GET /api/teams/:id/members — list all members of a team.
pub async fn list_members(
    State(db): State<Db>,
    Path(team_id): Path<String>,
) -> Result<Json<ListMembersResponse>, ApiErr> {
    let conn = db.conn();

    let exists: bool = conn
        .query_row(db::TEAM_EXISTS, [&team_id], |row| {
            row.get::<_, i64>(0).map(|c| c > 0)
        })
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

// ---------------------------------------------------------------------------
// Invitations
// ---------------------------------------------------------------------------

/// Check if the user is a team admin.
fn is_team_admin(conn: &rusqlite::Connection, team_id: &str, user_id: &str) -> bool {
    conn.query_row(
        db::TEAM_MEMBER_ROLE,
        rusqlite::params![team_id, user_id],
        |row| row.get::<_, String>(0),
    )
    .map(|role| role == "admin")
    .unwrap_or(false)
}

/// POST /api/teams/:id/invite — invite a member by email or OAuth identity.
pub async fn invite_member(
    State(db): State<Db>,
    user: AuthUser,
    Path(team_id): Path<String>,
    Json(req): Json<InviteRequest>,
) -> Result<(StatusCode, Json<InvitationResponse>), ApiErr> {
    let conn = db.conn();

    if !user.is_admin && !is_team_admin(&conn, &team_id, &user.user_id) {
        return Err(ApiErr::forbidden("team admin only"));
    }

    let email = req.email.as_deref().map(|e| e.trim().to_lowercase());
    let oauth_provider = req
        .oauth_provider
        .as_deref()
        .map(|p| p.trim().to_lowercase());
    let oauth_provider_username = req
        .oauth_provider_username
        .as_deref()
        .map(|u| u.trim().to_string());
    let role = req.role.as_deref().unwrap_or("member");

    let has_oauth = oauth_provider.is_some() && oauth_provider_username.is_some();
    if email.is_none() && !has_oauth {
        return Err(ApiErr::bad_request(
            "email or oauth_provider+oauth_provider_username required",
        ));
    }

    // Check for duplicate pending invitation
    let dup = if let Some(ref email) = email {
        conn.query_row(
            db::INVITATION_DUP_CHECK_EMAIL,
            rusqlite::params![&team_id, email],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    } else if let (Some(ref prov), Some(ref uname)) = (&oauth_provider, &oauth_provider_username) {
        conn.query_row(
            db::INVITATION_DUP_CHECK_OAUTH,
            rusqlite::params![&team_id, prov, uname],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    } else {
        0
    };

    if dup > 0 {
        return Err(ApiErr::conflict("invitation already pending"));
    }

    let id = Uuid::new_v4().to_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(7))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    conn.execute(
        db::INVITATION_INSERT,
        rusqlite::params![
            &id,
            &team_id,
            &email,
            &oauth_provider,
            &oauth_provider_username,
            &user.user_id,
            role,
            &expires_at
        ],
    )
    .map_err(|e| {
        tracing::error!("invite member: {e}");
        ApiErr::internal("failed to create invitation")
    })?;

    let team_name: String = conn
        .query_row(db::TEAM_NAME_BY_ID, [&team_id], |row| row.get(0))
        .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            id,
            team_id,
            team_name,
            email,
            oauth_provider,
            oauth_provider_username,
            invited_by_nickname: user.nickname,
            role: role.to_string(),
            status: "pending".to_string(),
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }),
    ))
}

/// GET /api/invitations — list pending invitations for the current user.
pub async fn list_invitations(
    State(db): State<Db>,
    user: AuthUser,
) -> Result<Json<ListInvitationsResponse>, ApiErr> {
    let conn = db.conn();
    let email = user.email.as_deref().unwrap_or("");

    let mut stmt = conn
        .prepare(&db::INVITATION_LIST_MY)
        .map_err(ApiErr::from_db("prepare invitations"))?;

    let invitations: Vec<InvitationResponse> = stmt
        .query_map(rusqlite::params![email, &user.user_id], |row| {
            Ok(InvitationResponse {
                id: row.get(0)?,
                team_id: row.get(1)?,
                team_name: row.get(2)?,
                email: row.get(3)?,
                oauth_provider: row.get(4)?,
                oauth_provider_username: row.get(5)?,
                invited_by_nickname: row.get(6)?,
                role: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .map_err(ApiErr::from_db("list invitations"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListInvitationsResponse { invitations }))
}

/// POST /api/invitations/:id/accept — accept an invitation.
pub async fn accept_invitation(
    State(db): State<Db>,
    user: AuthUser,
    Path(inv_id): Path<String>,
) -> Result<Json<AcceptInvitationResponse>, ApiErr> {
    let conn = db.conn();

    // Fetch invitation
    let inv = conn
        .query_row(db::INVITATION_LOOKUP, [&inv_id], |row| {
            Ok((
                row.get::<_, String>(0)?,         // id
                row.get::<_, String>(1)?,         // team_id
                row.get::<_, Option<String>>(2)?, // email
                row.get::<_, Option<String>>(3)?, // oauth_provider
                row.get::<_, Option<String>>(4)?, // oauth_provider_username
                row.get::<_, String>(5)?,         // role
                row.get::<_, String>(6)?,         // status
                row.get::<_, String>(7)?,         // expires_at
            ))
        })
        .map_err(|_| ApiErr::not_found("invitation not found"))?;

    let (_inv_id, team_id, inv_email, inv_prov, inv_uname, role, status, expires_at) = inv;

    if status != "pending" {
        return Err(ApiErr::bad_request("invitation is no longer pending"));
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if expires_at < now {
        return Err(ApiErr::bad_request("invitation has expired"));
    }

    // Verify ownership
    let email_match = inv_email
        .as_ref()
        .filter(|e| !e.is_empty())
        .map(|e| {
            user.email
                .as_ref()
                .map(|ue| ue.to_lowercase() == e.to_lowercase())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let oauth_match = match (&inv_prov, &inv_uname) {
        (Some(prov), Some(uname)) if !prov.is_empty() && !uname.is_empty() => conn
            .query_row(
                db::OAUTH_IDENTITY_MATCH,
                rusqlite::params![&user.user_id, prov, uname],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false),
        _ => false,
    };

    if !email_match && !oauth_match {
        return Err(ApiErr::forbidden("this invitation is not for you"));
    }

    // Check not already a member
    let already: bool = conn
        .query_row(
            db::TEAM_MEMBER_EXISTS,
            rusqlite::params![&team_id, &user.user_id],
            |row| row.get::<_, i64>(0).map(|c| c > 0),
        )
        .unwrap_or(false);

    if already {
        conn.execute(
            db::INVITATION_UPDATE_STATUS,
            rusqlite::params!["accepted", &inv_id],
        )
        .ok();
        return Err(ApiErr::conflict("already a member of this team"));
    }

    // Add as team member
    conn.execute(
        db::MEMBER_INSERT,
        rusqlite::params![&team_id, &user.user_id, &role],
    )
    .map_err(|e| {
        tracing::error!("accept invitation: {e}");
        ApiErr::internal("failed to join team")
    })?;

    // Mark invitation accepted
    conn.execute(
        db::INVITATION_UPDATE_STATUS,
        rusqlite::params!["accepted", &inv_id],
    )
    .ok();

    Ok(Json(AcceptInvitationResponse { team_id, role }))
}

/// POST /api/invitations/:id/decline — decline an invitation.
pub async fn decline_invitation(
    State(db): State<Db>,
    user: AuthUser,
    Path(inv_id): Path<String>,
) -> Result<Json<OkResponse>, ApiErr> {
    let conn = db.conn();

    // Fetch invitation
    let inv = conn
        .query_row(db::INVITATION_LOOKUP, [&inv_id], |row| {
            Ok((
                row.get::<_, Option<String>>(2)?, // email
                row.get::<_, Option<String>>(3)?, // oauth_provider
                row.get::<_, Option<String>>(4)?, // oauth_provider_username
                row.get::<_, String>(6)?,         // status
            ))
        })
        .map_err(|_| ApiErr::not_found("invitation not found"))?;

    let (inv_email, inv_prov, inv_uname, status) = inv;

    if status != "pending" {
        return Err(ApiErr::bad_request("invitation is no longer pending"));
    }

    // Verify ownership
    let email_match = inv_email
        .as_ref()
        .filter(|e| !e.is_empty())
        .map(|e| {
            user.email
                .as_ref()
                .map(|ue| ue.to_lowercase() == e.to_lowercase())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let oauth_match = match (&inv_prov, &inv_uname) {
        (Some(prov), Some(uname)) if !prov.is_empty() && !uname.is_empty() => conn
            .query_row(
                db::OAUTH_IDENTITY_MATCH,
                rusqlite::params![&user.user_id, prov, uname],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false),
        _ => false,
    };

    if !email_match && !oauth_match {
        return Err(ApiErr::forbidden("this invitation is not for you"));
    }

    conn.execute(
        db::INVITATION_UPDATE_STATUS,
        rusqlite::params!["declined", &inv_id],
    )
    .ok();

    Ok(Json(OkResponse { ok: true }))
}
