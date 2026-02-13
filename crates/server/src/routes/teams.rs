use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use opensession_api::{
    db, service, AcceptInvitationResponse, AddMemberRequest, CreateTeamRequest, InvitationResponse,
    InvitationStatus, InviteRequest, ListInvitationsResponse, ListMembersResponse,
    ListTeamsResponse, MemberResponse, OkResponse, SessionSummary, TeamDetailResponse,
    TeamResponse, TeamRole, TeamStatsQuery, TeamStatsResponse, TeamStatsTotals, ToolStats,
    UpdateTeamRequest, UserStats,
};

use crate::error::ApiErr;
use crate::routes::auth::AuthUser;
use crate::storage::{session_from_row, sq_execute, sq_query_map, sq_query_row, team_from_row, Db};

// ---------------------------------------------------------------------------
// Create team
// ---------------------------------------------------------------------------

/// POST /api/teams — create a new team. Creator is added as team admin.
pub async fn create_team(
    State(db): State<Db>,
    user: AuthUser,
    Json(req): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), ApiErr> {
    let name = service::validate_team_name(&req.name).map_err(ApiErr::from)?;

    let team_id = Uuid::new_v4().to_string();
    let is_public = req.is_public.unwrap_or(false);
    let conn = db.conn();

    sq_execute(
        &conn,
        db::teams::insert(
            &team_id,
            &name,
            req.description.as_deref(),
            is_public,
            &user.user_id,
        ),
    )
    .map_err(|e| {
        tracing::error!("create team: {e}");
        ApiErr::internal("failed to create team")
    })?;

    // Add creator as admin member
    sq_execute(
        &conn,
        db::teams::member_insert(&team_id, &user.user_id, TeamRole::Admin.as_str()),
    )
    .map_err(|e| {
        tracing::error!("add creator as member: {e}");
        ApiErr::internal("failed to create team")
    })?;

    let created_at: String =
        sq_query_row(&conn, db::teams::get_created_at(&team_id), |row| row.get(0))
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

    let teams: Vec<TeamResponse> =
        sq_query_map(&conn, db::teams::list_my(&user.user_id), team_from_row)
            .map_err(ApiErr::from_db("list teams"))?;

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

    let team = sq_query_row(&conn, db::teams::get_by_id(&id), team_from_row)
        .map_err(|_| ApiErr::not_found("team not found"))?;

    let member_count: i64 =
        sq_query_row(&conn, db::teams::member_count(&id), |row| row.get(0)).unwrap_or(0);

    let sessions: Vec<SessionSummary> =
        sq_query_map(&conn, db::sessions::list_by_team(&id, 50), session_from_row)
            .map_err(ApiErr::from_db("list team sessions"))?;

    Ok(Json(TeamDetailResponse {
        team,
        member_count,
        sessions,
    }))
}

// ---------------------------------------------------------------------------
// Team stats
// ---------------------------------------------------------------------------

/// GET /api/teams/:id/stats — aggregated team statistics.
pub async fn team_stats(
    State(db): State<Db>,
    Path(id): Path<String>,
    Query(query): Query<TeamStatsQuery>,
) -> Result<Json<TeamStatsResponse>, ApiErr> {
    let conn = db.conn();
    let time_range = query.time_range.unwrap_or_default();
    let time_filter = opensession_core::stats::sql::time_range_filter(time_range.as_str());

    // Totals
    let totals_sql = opensession_core::stats::sql::totals_query(time_filter);
    let totals = conn
        .query_row(&totals_sql, rusqlite::params![id], |row| {
            Ok(TeamStatsTotals {
                session_count: row.get(0)?,
                message_count: row.get(1)?,
                event_count: row.get(2)?,
                tool_call_count: 0,
                duration_seconds: row.get(3)?,
                total_input_tokens: row.get(4)?,
                total_output_tokens: row.get(5)?,
            })
        })
        .unwrap_or(TeamStatsTotals {
            session_count: 0,
            message_count: 0,
            event_count: 0,
            tool_call_count: 0,
            duration_seconds: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        });

    // By user
    let by_user_sql = opensession_core::stats::sql::by_user_query(time_filter);
    let mut stmt = conn.prepare(&by_user_sql).map_err(|e| {
        tracing::error!("prepare by_user: {e}");
        ApiErr::internal("failed to query stats")
    })?;
    let by_user: Vec<UserStats> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(UserStats {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                session_count: row.get(2)?,
                message_count: row.get(3)?,
                event_count: row.get(4)?,
                duration_seconds: row.get(5)?,
                total_input_tokens: row.get(6)?,
                total_output_tokens: row.get(7)?,
            })
        })
        .map_err(|e| {
            tracing::error!("query by_user: {e}");
            ApiErr::internal("failed to query stats")
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| {
            tracing::error!("collect by_user: {e}");
            ApiErr::internal("failed to query stats")
        })?;

    // By tool
    let by_tool_sql = opensession_core::stats::sql::by_tool_query(time_filter);
    let mut stmt = conn.prepare(&by_tool_sql).map_err(|e| {
        tracing::error!("prepare by_tool: {e}");
        ApiErr::internal("failed to query stats")
    })?;
    let by_tool: Vec<ToolStats> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(ToolStats {
                tool: row.get(0)?,
                session_count: row.get(1)?,
                message_count: row.get(2)?,
                event_count: row.get(3)?,
                duration_seconds: row.get(4)?,
                total_input_tokens: row.get(5)?,
                total_output_tokens: row.get(6)?,
            })
        })
        .map_err(|e| {
            tracing::error!("query by_tool: {e}");
            ApiErr::internal("failed to query stats")
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| {
            tracing::error!("collect by_tool: {e}");
            ApiErr::internal("failed to query stats")
        })?;

    Ok(Json(TeamStatsResponse {
        team_id: id,
        time_range,
        totals,
        by_user,
        by_tool,
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
    let conn = db.conn();

    if !is_team_admin(&conn, &id, &user.user_id) {
        return Err(ApiErr::forbidden("team admin only"));
    }

    // Verify team exists
    let exists: bool = sq_query_row(&conn, db::teams::exists(&id), |row| {
        row.get::<_, i64>(0).map(|c| c > 0)
    })
    .unwrap_or(false);

    if !exists {
        return Err(ApiErr::not_found("team not found"));
    }

    if let Some(ref name) = req.name {
        let name = service::validate_team_name(name).map_err(ApiErr::from)?;
        sq_execute(&conn, db::teams::update_name(&id, &name))
            .map_err(ApiErr::from_db("update team name"))?;
    }

    if let Some(ref desc) = req.description {
        sq_execute(&conn, db::teams::update_description(&id, desc))
            .map_err(ApiErr::from_db("update team description"))?;
    }

    if let Some(is_public) = req.is_public {
        sq_execute(&conn, db::teams::update_visibility(&id, is_public))
            .map_err(ApiErr::from_db("update team visibility"))?;
    }

    let team = sq_query_row(&conn, db::teams::get_by_id(&id), team_from_row)
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
    let conn = db.conn();

    if !is_team_admin(&conn, &team_id, &user.user_id) {
        return Err(ApiErr::forbidden("team admin only"));
    }

    // Look up user by nickname
    let target = sq_query_row(&conn, db::users::get_by_nickname(&req.nickname), |row| {
        row.get::<_, String>(0)
    })
    .map_err(|_| ApiErr::not_found("user not found"))?;

    // Check not already a member
    let already: bool = sq_query_row(&conn, db::teams::member_exists(&team_id, &target), |row| {
        row.get::<_, i64>(0).map(|c| c > 0)
    })
    .unwrap_or(false);

    if already {
        return Err(ApiErr::conflict("already a member"));
    }

    sq_execute(
        &conn,
        db::teams::member_insert(&team_id, &target, TeamRole::Member.as_str()),
    )
    .map_err(|e| {
        tracing::error!("add member: {e}");
        ApiErr::internal("failed to add member")
    })?;

    let joined_at: String = sq_query_row(
        &conn,
        db::teams::member_joined_at(&team_id, &target),
        |row| row.get(0),
    )
    .unwrap_or_default();

    Ok((
        StatusCode::CREATED,
        Json(MemberResponse {
            user_id: target,
            nickname: req.nickname,
            role: TeamRole::Member,
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
    let conn = db.conn();

    if !is_team_admin(&conn, &team_id, &user.user_id) {
        return Err(ApiErr::forbidden("team admin only"));
    }
    let affected =
        sq_execute(&conn, db::teams::member_delete(&team_id, &user_id)).map_err(|e| {
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

    let exists: bool = sq_query_row(&conn, db::teams::exists(&team_id), |row| {
        row.get::<_, i64>(0).map(|c| c > 0)
    })
    .unwrap_or(false);

    if !exists {
        return Err(ApiErr::not_found("team not found"));
    }

    let members: Vec<MemberResponse> =
        sq_query_map(&conn, db::teams::member_list(&team_id), |row| {
            let role_str: String = row.get(2)?;
            Ok(MemberResponse {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                role: if role_str == "admin" {
                    TeamRole::Admin
                } else {
                    TeamRole::Member
                },
                joined_at: row.get(3)?,
            })
        })
        .map_err(ApiErr::from_db("list members"))?;

    Ok(Json(ListMembersResponse { members }))
}

// ---------------------------------------------------------------------------
// Invitations
// ---------------------------------------------------------------------------

/// Check if the user is a team admin.
fn is_team_admin(conn: &rusqlite::Connection, team_id: &str, user_id: &str) -> bool {
    sq_query_row(conn, db::teams::member_role(team_id, user_id), |row| {
        row.get::<_, String>(0)
    })
    .map(|role| role == TeamRole::Admin.as_str())
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

    if !is_team_admin(&conn, &team_id, &user.user_id) {
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
    let role = req.role.unwrap_or(TeamRole::Member);

    let has_oauth = oauth_provider.is_some() && oauth_provider_username.is_some();
    if email.is_none() && !has_oauth {
        return Err(ApiErr::bad_request(
            "email or oauth_provider+oauth_provider_username required",
        ));
    }

    // Check for duplicate pending invitation
    let dup = if let Some(ref email) = email {
        sq_query_row(
            &conn,
            db::invitations::dup_check_email(&team_id, email),
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
    } else if let (Some(ref prov), Some(ref uname)) = (&oauth_provider, &oauth_provider_username) {
        sq_query_row(
            &conn,
            db::invitations::dup_check_oauth(&team_id, prov, uname),
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

    sq_execute(
        &conn,
        db::invitations::insert(
            &id,
            &team_id,
            email.as_deref(),
            oauth_provider.as_deref(),
            oauth_provider_username.as_deref(),
            &user.user_id,
            role.as_str(),
            &expires_at,
        ),
    )
    .map_err(|e| {
        tracing::error!("invite member: {e}");
        ApiErr::internal("failed to create invitation")
    })?;

    let team_name: String =
        sq_query_row(&conn, db::teams::get_name(&team_id), |row| row.get(0)).unwrap_or_default();

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
            role,
            status: InvitationStatus::Pending,
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

    let invitations: Vec<InvitationResponse> = sq_query_map(
        &conn,
        db::invitations::list_my(email, &user.user_id),
        |row| {
            let role_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            Ok(InvitationResponse {
                id: row.get(0)?,
                team_id: row.get(1)?,
                team_name: row.get(2)?,
                email: row.get(3)?,
                oauth_provider: row.get(4)?,
                oauth_provider_username: row.get(5)?,
                invited_by_nickname: row.get(6)?,
                role: if role_str == "admin" {
                    TeamRole::Admin
                } else {
                    TeamRole::Member
                },
                status: match status_str.as_str() {
                    "accepted" => InvitationStatus::Accepted,
                    "declined" => InvitationStatus::Declined,
                    _ => InvitationStatus::Pending,
                },
                created_at: row.get(9)?,
            })
        },
    )
    .map_err(ApiErr::from_db("list invitations"))?;

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
    let inv = sq_query_row(&conn, db::invitations::lookup(&inv_id), |row| {
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

    let (_inv_id, team_id, inv_email, inv_prov, inv_uname, role_str, status, expires_at) = inv;

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
        (Some(prov), Some(uname)) if !prov.is_empty() && !uname.is_empty() => sq_query_row(
            &conn,
            db::oauth::identity_match(&user.user_id, prov, uname),
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
    let already: bool = sq_query_row(
        &conn,
        db::teams::member_exists(&team_id, &user.user_id),
        |row| row.get::<_, i64>(0).map(|c| c > 0),
    )
    .unwrap_or(false);

    if already {
        sq_execute(
            &conn,
            db::invitations::update_status(&inv_id, InvitationStatus::Accepted.as_str()),
        )
        .ok();
        return Err(ApiErr::conflict("already a member of this team"));
    }

    let role = if role_str == "admin" {
        TeamRole::Admin
    } else {
        TeamRole::Member
    };

    // Add as team member
    sq_execute(
        &conn,
        db::teams::member_insert(&team_id, &user.user_id, role.as_str()),
    )
    .map_err(|e| {
        tracing::error!("accept invitation: {e}");
        ApiErr::internal("failed to join team")
    })?;

    // Mark invitation accepted
    sq_execute(
        &conn,
        db::invitations::update_status(&inv_id, InvitationStatus::Accepted.as_str()),
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
    let inv = sq_query_row(&conn, db::invitations::lookup(&inv_id), |row| {
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
        (Some(prov), Some(uname)) if !prov.is_empty() && !uname.is_empty() => sq_query_row(
            &conn,
            db::oauth::identity_match(&user.user_id, prov, uname),
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false),
        _ => false,
    };

    if !email_match && !oauth_match {
        return Err(ApiErr::forbidden("this invitation is not for you"));
    }

    sq_execute(
        &conn,
        db::invitations::update_status(&inv_id, InvitationStatus::Declined.as_str()),
    )
    .ok();

    Ok(Json(OkResponse { ok: true }))
}
