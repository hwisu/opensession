use worker::*;

use opensession_api::db;
use opensession_api::{
    AcceptInvitationResponse, AddMemberRequest, CreateTeamRequest, InvitationResponse,
    InvitationStatus, InviteRequest, ListInvitationsResponse, ListMembersResponse,
    ListTeamsResponse, MemberResponse, SessionSummary, TeamDetailResponse, TeamResponse, TeamRole,
    TeamStatsResponse, TeamStatsTotals, TimeRange, ToolStats, UpdateTeamRequest, UserStats,
};

use opensession_api::{service, ServiceError};

use crate::db_helpers::values_to_js;
use crate::error::IntoErrResponse;
use crate::storage;

impl From<storage::TeamRow> for TeamResponse {
    fn from(t: storage::TeamRow) -> Self {
        Self {
            id: t.id,
            name: t.name,
            description: t.description,
            is_public: t.is_public,
            created_by: t.created_by,
            created_at: t.created_at,
        }
    }
}

impl From<storage::MemberRow> for MemberResponse {
    fn from(m: storage::MemberRow) -> Self {
        Self {
            user_id: m.user_id,
            nickname: m.nickname,
            role: if m.role == "admin" {
                TeamRole::Admin
            } else {
                TeamRole::Member
            },
            joined_at: m.joined_at,
        }
    }
}

/// Check if a user is a team admin (has role='admin' in team_members).
async fn is_team_admin(d1: &D1Database, team_id: &str, user_id: &str) -> Result<bool> {
    let (sql, values) = db::teams::member_role(team_id, user_id);
    let row = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::RoleRow>(None)
        .await?;
    Ok(row
        .map(|r| r.role == TeamRole::Admin.as_str())
        .unwrap_or(false))
}

/// POST /api/teams — create team (any authenticated user)
pub async fn create(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let body: CreateTeamRequest = req.json().await?;
    let name = match service::validate_team_name(&body.name) {
        Ok(n) => n,
        Err(e) => return e.into_err_response(),
    };

    let team_id = uuid::Uuid::new_v4().to_string();
    let is_public = body.is_public.unwrap_or(false);
    let d1 = storage::get_d1(&ctx.env)?;

    // Create the team using sea-query INSERT
    let (sql, values) = db::teams::insert(
        &team_id,
        &name,
        body.description.as_deref(),
        is_public,
        &user.id,
    );
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    // Add creator as admin member
    let (sql, values) = db::teams::member_insert(&team_id, &user.id, TeamRole::Admin.as_str());
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    let (sql, values) = db::teams::get_created_at(&team_id);
    let created_at = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CreatedAtRow>(None)
        .await?
        .map(|r| r.created_at)
        .unwrap_or_default();

    let mut resp = Response::from_json(&TeamResponse {
        id: team_id,
        name,
        description: body.description,
        is_public,
        created_by: user.id,
        created_at,
    })?;
    resp = resp.with_status(201);
    Ok(resp)
}

/// GET /api/teams — list my teams
pub async fn list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let d1 = storage::get_d1(&ctx.env)?;
    let (sql, values) = db::teams::list_my(&user.id);
    let results = d1.prepare(&sql).bind(&values_to_js(&values))?.all().await?;

    let teams: Vec<TeamResponse> = results
        .results::<storage::TeamRow>()?
        .into_iter()
        .map(TeamResponse::from)
        .collect();

    Response::from_json(&ListTeamsResponse { teams })
}

/// GET /api/teams/:id — get team detail + sessions
pub async fn get(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    let (sql, values) = db::teams::get_by_id(id);
    let team = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::TeamRow>(None)
        .await?;

    let team = match team {
        Some(t) => TeamResponse::from(t),
        None => {
            return ServiceError::NotFound("team not found".into()).into_err_response();
        }
    };

    let (sql, values) = db::teams::member_count(id);
    let member_count = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    let (sql, values) = db::sessions::list_by_team(id, 50);
    let sessions_result = d1.prepare(&sql).bind(&values_to_js(&values))?.all().await?;

    let sessions: Vec<SessionSummary> = sessions_result
        .results::<storage::SessionRow>()?
        .into_iter()
        .map(SessionSummary::from)
        .collect();

    Response::from_json(&TeamDetailResponse {
        team,
        member_count,
        sessions,
    })
}

/// PUT /api/teams/:id — update team (site admin or team admin)
pub async fn update(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    if !is_team_admin(&d1, id, &user.id).await? {
        return ServiceError::Forbidden("team admin only".into()).into_err_response();
    }

    let body: UpdateTeamRequest = req.json().await?;

    // Verify team exists
    let (sql, values) = db::teams::exists(id);
    let exists = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if exists == 0 {
        return ServiceError::NotFound("team not found".into()).into_err_response();
    }

    if let Some(ref name) = body.name {
        let name = match service::validate_team_name(name) {
            Ok(n) => n,
            Err(e) => return e.into_err_response(),
        };
        let (sql, values) = db::teams::update_name(id, &name);
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
    }

    if let Some(ref desc) = body.description {
        let (sql, values) = db::teams::update_description(id, desc);
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
    }

    if let Some(is_public) = body.is_public {
        let (sql, values) = db::teams::update_visibility(id, is_public);
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
    }

    let (sql, values) = db::teams::get_by_id(id);
    let team = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::TeamRow>(None)
        .await?
        .ok_or_else(|| Error::from("team not found"))?;

    Response::from_json(&TeamResponse::from(team))
}

/// GET /api/teams/:id/members — list team members
pub async fn list_members(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    // Verify team exists
    let (sql, values) = db::teams::exists(id);
    let exists = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if exists == 0 {
        return ServiceError::NotFound("team not found".into()).into_err_response();
    }

    let (sql, values) = db::teams::member_list(id);
    let results = d1.prepare(&sql).bind(&values_to_js(&values))?.all().await?;

    let members: Vec<MemberResponse> = results
        .results::<storage::MemberRow>()?
        .into_iter()
        .map(MemberResponse::from)
        .collect();

    Response::from_json(&ListMembersResponse { members })
}

/// POST /api/teams/:id/members — add member (site admin or team admin, by nickname)
pub async fn add_member(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let team_id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    if !is_team_admin(&d1, team_id, &user.id).await? {
        return ServiceError::Forbidden("team admin only".into()).into_err_response();
    }

    let body: AddMemberRequest = req.json().await?;

    let (sql, values) = db::users::get_by_nickname(&body.nickname);
    let target = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::UserIdRow>(None)
        .await?;

    let target_id = match target {
        Some(u) => u.id,
        None => {
            return ServiceError::NotFound("user not found".into()).into_err_response();
        }
    };

    // Check not already a member
    let (sql, values) = db::teams::member_exists(team_id, &target_id);
    let already = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if already > 0 {
        return ServiceError::Conflict("already a member".into()).into_err_response();
    }

    let (sql, values) = db::teams::member_insert(team_id, &target_id, TeamRole::Member.as_str());
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    let (sql, values) = db::teams::member_joined_at(team_id, &target_id);
    let joined_at = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::JoinedAtRow>(None)
        .await?
        .map(|r| r.joined_at)
        .unwrap_or_default();

    let mut resp = Response::from_json(&MemberResponse {
        user_id: target_id,
        nickname: body.nickname,
        role: TeamRole::Member,
        joined_at,
    })?;
    resp = resp.with_status(201);
    Ok(resp)
}

/// DELETE /api/teams/:team_id/members/:user_id — remove member (site admin or team admin)
pub async fn remove_member(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let team_id = ctx
        .param("team_id")
        .ok_or_else(|| Error::from("Missing team_id"))?;
    let user_id = ctx
        .param("user_id")
        .ok_or_else(|| Error::from("Missing user_id"))?;

    let d1 = storage::get_d1(&ctx.env)?;

    if !is_team_admin(&d1, team_id, &user.id).await? {
        return ServiceError::Forbidden("team admin only".into()).into_err_response();
    }

    // D1 doesn't return affected rows directly, check existence first
    let (sql, values) = db::teams::member_exists(team_id, user_id);
    let exists = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if exists == 0 {
        return ServiceError::NotFound("member not found".into()).into_err_response();
    }

    let (sql, values) = db::teams::member_delete(team_id, user_id);
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Ok(Response::empty()?.with_status(204))
}

// ── Stats ───────────────────────────────────────────────────────────────────

/// GET /api/teams/:id/stats — aggregated team statistics
pub async fn stats(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;
    let d1 = storage::get_d1(&ctx.env)?;

    // Parse query params
    let url = req.url()?;
    let time_range: TimeRange = url
        .query_pairs()
        .find(|(k, _)| k == "time_range")
        .and_then(|(_, v)| serde_json::from_value(serde_json::Value::String(v.into_owned())).ok())
        .unwrap_or_default();

    let time_filter = opensession_core::stats::sql::time_range_filter(time_range.as_str());

    // Totals
    let totals_sql = opensession_core::stats::sql::totals_query(time_filter);
    let totals_row = d1
        .prepare(&totals_sql)
        .bind(&[id.into()])?
        .first::<storage::StatsTotalsRow>(None)
        .await?;

    let totals = match totals_row {
        Some(r) => TeamStatsTotals {
            session_count: r.session_count,
            message_count: r.message_count,
            event_count: r.event_count,
            tool_call_count: 0,
            duration_seconds: r.duration_seconds,
            total_input_tokens: r.total_input_tokens,
            total_output_tokens: r.total_output_tokens,
        },
        None => TeamStatsTotals {
            session_count: 0,
            message_count: 0,
            event_count: 0,
            tool_call_count: 0,
            duration_seconds: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        },
    };

    // By user
    let by_user_sql = opensession_core::stats::sql::by_user_query(time_filter);
    let by_user_results = d1.prepare(&by_user_sql).bind(&[id.into()])?.all().await?;
    let by_user: Vec<UserStats> = by_user_results
        .results::<storage::UserStatsRow>()?
        .into_iter()
        .map(|r| UserStats {
            user_id: r.user_id,
            nickname: r.nickname,
            session_count: r.session_count,
            message_count: r.message_count,
            event_count: r.event_count,
            duration_seconds: r.duration_seconds,
            total_input_tokens: r.total_input_tokens,
            total_output_tokens: r.total_output_tokens,
        })
        .collect();

    // By tool
    let by_tool_sql = opensession_core::stats::sql::by_tool_query(time_filter);
    let by_tool_results = d1.prepare(&by_tool_sql).bind(&[id.into()])?.all().await?;
    let by_tool: Vec<ToolStats> = by_tool_results
        .results::<storage::ToolStatsRow>()?
        .into_iter()
        .map(|r| ToolStats {
            tool: r.tool,
            session_count: r.session_count,
            message_count: r.message_count,
            event_count: r.event_count,
            duration_seconds: r.duration_seconds,
            total_input_tokens: r.total_input_tokens,
            total_output_tokens: r.total_output_tokens,
        })
        .collect();

    Response::from_json(&TeamStatsResponse {
        team_id: id.to_string(),
        time_range,
        totals,
        by_user,
        by_tool,
    })
}

// ── Invitations ─────────────────────────────────────────────────────────────

/// Check if user email matches invitation email (case-insensitive).
fn email_matches(user_email: Option<&str>, inv_email: Option<&str>) -> bool {
    match (user_email, inv_email) {
        (Some(ue), Some(ie)) if !ie.is_empty() => ue.to_lowercase() == ie.to_lowercase(),
        _ => false,
    }
}

/// Check if user has a matching OAuth identity for the invitation.
async fn oauth_matches(
    d1: &D1Database,
    user_id: &str,
    provider: Option<&str>,
    username: Option<&str>,
) -> Result<bool> {
    match (provider, username) {
        (Some(prov), Some(uname)) if !prov.is_empty() && !uname.is_empty() => {
            let (sql, values) = db::oauth::identity_match(user_id, prov, uname);
            Ok(d1
                .prepare(&sql)
                .bind(&values_to_js(&values))?
                .first::<storage::CountRow>(None)
                .await?
                .map(|r| r.count > 0)
                .unwrap_or(false))
        }
        _ => Ok(false),
    }
}

impl From<storage::InvitationRow> for InvitationResponse {
    fn from(r: storage::InvitationRow) -> Self {
        Self {
            id: r.id,
            team_id: r.team_id,
            team_name: r.team_name,
            email: r.email,
            oauth_provider: r.oauth_provider,
            oauth_provider_username: r.oauth_provider_username,
            invited_by_nickname: r.invited_by_nickname,
            role: if r.role == "admin" {
                TeamRole::Admin
            } else {
                TeamRole::Member
            },
            status: match r.status.as_str() {
                "accepted" => InvitationStatus::Accepted,
                "declined" => InvitationStatus::Declined,
                _ => InvitationStatus::Pending,
            },
            created_at: r.created_at,
        }
    }
}

/// POST /api/teams/:id/invite — invite member by email or GitHub username
pub async fn invite_member(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let team_id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;
    let d1 = storage::get_d1(&ctx.env)?;

    if !is_team_admin(&d1, team_id, &user.id).await? {
        return ServiceError::Forbidden("team admin only".into()).into_err_response();
    }

    let body: InviteRequest = req.json().await?;

    let email = body.email.as_deref().map(|e| e.trim().to_lowercase());
    let oauth_provider = body
        .oauth_provider
        .as_deref()
        .map(|p| p.trim().to_lowercase());
    let oauth_provider_username = body
        .oauth_provider_username
        .as_deref()
        .map(|u| u.trim().to_string());
    let role = body.role.unwrap_or(TeamRole::Member);

    let has_oauth = oauth_provider.is_some() && oauth_provider_username.is_some();
    if email.is_none() && !has_oauth {
        return ServiceError::BadRequest(
            "email or oauth_provider+oauth_provider_username required".into(),
        )
        .into_err_response();
    }

    // Check for duplicate pending invitation
    let dup_check = if let Some(ref email) = email {
        let (sql, values) = db::invitations::dup_check_email(team_id, email);
        d1.prepare(&sql)
            .bind(&values_to_js(&values))?
            .first::<storage::CountRow>(None)
            .await?
    } else if let (Some(ref prov), Some(ref uname)) = (&oauth_provider, &oauth_provider_username) {
        let (sql, values) = db::invitations::dup_check_oauth(team_id, prov, uname);
        d1.prepare(&sql)
            .bind(&values_to_js(&values))?
            .first::<storage::CountRow>(None)
            .await?
    } else {
        None
    };

    if dup_check.map(|r| r.count).unwrap_or(0) > 0 {
        return ServiceError::Conflict("invitation already pending".into()).into_err_response();
    }

    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(7))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let (sql, values) = db::invitations::insert(
        &id,
        team_id,
        email.as_deref(),
        oauth_provider.as_deref(),
        oauth_provider_username.as_deref(),
        &user.id,
        role.as_str(),
        &expires_at,
    );
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    // Fetch team name for the response
    let (sql, values) = db::teams::get_name(team_id);
    let team_name = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::TeamNameRow>(None)
        .await?
        .map(|r| r.name)
        .unwrap_or_default();

    let mut resp = Response::from_json(&InvitationResponse {
        id,
        team_id: team_id.to_string(),
        team_name,
        email,
        oauth_provider,
        oauth_provider_username,
        invited_by_nickname: user.nickname,
        role,
        status: InvitationStatus::Pending,
        created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    })?;
    resp = resp.with_status(201);
    Ok(resp)
}

/// GET /api/invitations — list my pending invitations
pub async fn list_invitations(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let d1 = storage::get_d1(&ctx.env)?;

    let email = user.email.as_deref().unwrap_or("");

    let (sql, values) = db::invitations::list_my(email, &user.id);
    let results = d1.prepare(&sql).bind(&values_to_js(&values))?.all().await?;

    let invitations: Vec<InvitationResponse> = results
        .results::<storage::InvitationRow>()?
        .into_iter()
        .map(InvitationResponse::from)
        .collect();

    Response::from_json(&ListInvitationsResponse { invitations })
}

/// POST /api/invitations/:id/accept — accept an invitation
pub async fn accept_invitation(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let inv_id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;
    let d1 = storage::get_d1(&ctx.env)?;

    // Fetch invitation
    let (sql, values) = db::invitations::lookup(inv_id);
    let inv = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::InvitationLookupRow>(None)
        .await?;

    let inv = match inv {
        Some(i) => i,
        None => return ServiceError::NotFound("invitation not found".into()).into_err_response(),
    };

    if inv.status != "pending" {
        return ServiceError::BadRequest("invitation is no longer pending".into())
            .into_err_response();
    }

    // Check expiry
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if inv.expires_at < now {
        return ServiceError::BadRequest("invitation has expired".into()).into_err_response();
    }

    // Verify the invitation belongs to this user
    let is_match = email_matches(user.email.as_deref(), inv.email.as_deref())
        || oauth_matches(
            &d1,
            &user.id,
            inv.oauth_provider.as_deref(),
            inv.oauth_provider_username.as_deref(),
        )
        .await?;

    if !is_match {
        return ServiceError::Forbidden("this invitation is not for you".into())
            .into_err_response();
    }

    // Check not already a member
    let (sql, values) = db::teams::member_exists(&inv.team_id, &user.id);
    let already = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::CountRow>(None)
        .await?
        .map(|r| r.count)
        .unwrap_or(0);

    if already > 0 {
        // Already a member — mark invitation accepted and return
        let (sql, values) =
            db::invitations::update_status(inv_id, InvitationStatus::Accepted.as_str());
        d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;
        return ServiceError::Conflict("already a member of this team".into()).into_err_response();
    }

    let role = if inv.role == "admin" {
        TeamRole::Admin
    } else {
        TeamRole::Member
    };

    // Add as team member
    let (sql, values) = db::teams::member_insert(&inv.team_id, &user.id, role.as_str());
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    // Mark invitation accepted
    let (sql, values) = db::invitations::update_status(inv_id, InvitationStatus::Accepted.as_str());
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Response::from_json(&AcceptInvitationResponse {
        team_id: inv.team_id,
        role,
    })
}

/// POST /api/invitations/:id/decline — decline an invitation
pub async fn decline_invitation(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user = match storage::require_auth(&req, &ctx.env).await {
        Ok(u) => u,
        Err(resp) => return Ok(resp),
    };

    let inv_id = ctx.param("id").ok_or_else(|| Error::from("Missing id"))?;
    let d1 = storage::get_d1(&ctx.env)?;

    // Fetch invitation
    let (sql, values) = db::invitations::lookup(inv_id);
    let inv = d1
        .prepare(&sql)
        .bind(&values_to_js(&values))?
        .first::<storage::InvitationLookupRow>(None)
        .await?;

    let inv = match inv {
        Some(i) => i,
        None => return ServiceError::NotFound("invitation not found".into()).into_err_response(),
    };

    if inv.status != "pending" {
        return ServiceError::BadRequest("invitation is no longer pending".into())
            .into_err_response();
    }

    // Verify ownership
    let is_match = email_matches(user.email.as_deref(), inv.email.as_deref())
        || oauth_matches(
            &d1,
            &user.id,
            inv.oauth_provider.as_deref(),
            inv.oauth_provider_username.as_deref(),
        )
        .await?;

    if !is_match {
        return ServiceError::Forbidden("this invitation is not for you".into())
            .into_err_response();
    }

    let (sql, values) = db::invitations::update_status(inv_id, InvitationStatus::Declined.as_str());
    d1.prepare(&sql).bind(&values_to_js(&values))?.run().await?;

    Response::from_json(&opensession_api::OkResponse { ok: true })
}
