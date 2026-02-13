use anyhow::{ensure, Context, Result};
use uuid::Uuid;

use opensession_api::{
    AddMemberRequest, CreateTeamRequest, TeamResponse, UpdateTeamRequest, UploadRequest,
};

use crate::client::TestContext;
use crate::fixtures;

/// Admin POST /api/teams → 201.
pub async fn create_team(ctx: &TestContext) -> Result<()> {
    let admin = ctx.admin().context("admin not set")?;
    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);

    let resp = ctx
        .post_json_authed(
            "/teams",
            &admin.access_token,
            &CreateTeamRequest {
                name: name.clone(),
                description: Some("test team".into()),
                is_public: Some(false),
            },
        )
        .await?;
    ensure!(resp.status() == 201, "expected 201, got {}", resp.status());
    let team: TeamResponse = resp.json().await?;
    ensure!(team.name == name);
    Ok(())
}

/// Any authenticated user can create a team.
pub async fn create_team_any_user(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);

    let resp = ctx
        .post_json_authed(
            "/teams",
            &user.access_token,
            &CreateTeamRequest {
                name: name.clone(),
                description: None,
                is_public: None,
            },
        )
        .await?;
    ensure!(resp.status() == 201, "expected 201, got {}", resp.status());
    let team: TeamResponse = resp.json().await?;
    ensure!(team.name == name);
    Ok(())
}

/// GET /api/teams shows user's teams.
pub async fn list_teams(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    let resp = ctx.get_authed("/teams", &user.access_token).await?;
    ensure!(resp.status() == 200);

    let body: serde_json::Value = resp.json().await?;
    let teams = body["teams"].as_array().expect("expected teams array");
    ensure!(
        teams.iter().any(|t| t["id"].as_str() == Some(&team_id)),
        "expected user's team in list"
    );
    Ok(())
}

/// GET /api/teams/{id} → detail with member_count.
pub async fn get_team_detail(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    let resp = ctx
        .get_authed(&format!("/teams/{team_id}"), &user.access_token)
        .await?;
    ensure!(resp.status() == 200);

    let detail: serde_json::Value = resp.json().await?;
    ensure!(detail["id"] == team_id);
    ensure!(
        detail["member_count"].as_i64().unwrap() >= 1,
        "expected at least 1 member"
    );
    Ok(())
}

/// Admin PUT → name updated.
pub async fn update_team(ctx: &TestContext) -> Result<()> {
    let admin = ctx.admin().context("admin not set")?;
    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);

    let resp = ctx
        .post_json_authed(
            "/teams",
            &admin.access_token,
            &CreateTeamRequest {
                name,
                description: None,
                is_public: None,
            },
        )
        .await?;
    let team: TeamResponse = resp.json().await?;

    let new_name = format!("updated-{}", &Uuid::new_v4().to_string()[..8]);
    let resp = ctx
        .put_json_authed(
            &format!("/teams/{}", team.id),
            &admin.access_token,
            &UpdateTeamRequest {
                name: Some(new_name.clone()),
                description: None,
                is_public: None,
            },
        )
        .await?;
    ensure!(resp.status() == 200);

    let updated: TeamResponse = resp.json().await?;
    ensure!(updated.name == new_name);
    Ok(())
}

/// Non-admin PUT → 403.
pub async fn update_team_non_admin(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    let resp = ctx
        .put_json_authed(
            &format!("/teams/{team_id}"),
            &user.access_token,
            &UpdateTeamRequest {
                name: Some("hacked".into()),
                description: None,
                is_public: None,
            },
        )
        .await?;
    ensure!(resp.status() == 403, "expected 403, got {}", resp.status());
    Ok(())
}

/// POST /api/teams/{id}/members → 201.
pub async fn add_member(ctx: &TestContext) -> Result<()> {
    let admin = ctx.admin().context("admin not set")?;
    let user = ctx.register_user().await?;

    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);
    let resp = ctx
        .post_json_authed(
            "/teams",
            &admin.access_token,
            &CreateTeamRequest {
                name,
                description: None,
                is_public: None,
            },
        )
        .await?;
    let team: TeamResponse = resp.json().await?;

    let resp = ctx
        .post_json_authed(
            &format!("/teams/{}/members", team.id),
            &admin.access_token,
            &AddMemberRequest {
                nickname: user.nickname.clone(),
            },
        )
        .await?;
    ensure!(resp.status() == 201, "expected 201, got {}", resp.status());
    Ok(())
}

/// Same user twice → 409.
pub async fn add_member_duplicate(ctx: &TestContext) -> Result<()> {
    let admin = ctx.admin().context("admin not set")?;
    let user = ctx.register_user().await?;

    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);
    let resp = ctx
        .post_json_authed(
            "/teams",
            &admin.access_token,
            &CreateTeamRequest {
                name,
                description: None,
                is_public: None,
            },
        )
        .await?;
    let team: TeamResponse = resp.json().await?;

    // First add
    ctx.post_json_authed(
        &format!("/teams/{}/members", team.id),
        &admin.access_token,
        &AddMemberRequest {
            nickname: user.nickname.clone(),
        },
    )
    .await?;

    // Second add → conflict
    let resp = ctx
        .post_json_authed(
            &format!("/teams/{}/members", team.id),
            &admin.access_token,
            &AddMemberRequest {
                nickname: user.nickname.clone(),
            },
        )
        .await?;
    ensure!(resp.status() == 409, "expected 409, got {}", resp.status());
    Ok(())
}

/// DELETE → 204, no longer in members.
pub async fn remove_member(ctx: &TestContext) -> Result<()> {
    let admin = ctx.admin().context("admin not set")?;
    let user = ctx.register_user().await?;

    let name = format!("team-{}", &Uuid::new_v4().to_string()[..8]);
    let resp = ctx
        .post_json_authed(
            "/teams",
            &admin.access_token,
            &CreateTeamRequest {
                name,
                description: None,
                is_public: None,
            },
        )
        .await?;
    let team: TeamResponse = resp.json().await?;

    // Add member
    ctx.post_json_authed(
        &format!("/teams/{}/members", team.id),
        &admin.access_token,
        &AddMemberRequest {
            nickname: user.nickname.clone(),
        },
    )
    .await?;

    // Remove member
    let resp = ctx
        .delete_authed(
            &format!("/teams/{}/members/{}", team.id, user.user_id),
            &admin.access_token,
        )
        .await?;
    ensure!(resp.status() == 204, "expected 204, got {}", resp.status());

    // Verify not in members list
    let resp = ctx
        .get_authed(&format!("/teams/{}/members", team.id), &admin.access_token)
        .await?;
    let body: serde_json::Value = resp.json().await?;
    let members = body["members"].as_array().expect("members array");
    ensure!(
        !members
            .iter()
            .any(|m| m["user_id"].as_str() == Some(&user.user_id)),
        "expected user removed from members"
    );
    Ok(())
}

/// Non-admin DELETE → 403.
pub async fn remove_member_non_admin(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;
    let other = ctx.register_user().await?;

    let admin = ctx.admin().context("admin not set")?;
    ctx.post_json_authed(
        &format!("/teams/{team_id}/members"),
        &admin.access_token,
        &AddMemberRequest {
            nickname: other.nickname.clone(),
        },
    )
    .await?;

    // Non-admin tries to remove
    let resp = ctx
        .delete_authed(
            &format!("/teams/{team_id}/members/{}", other.user_id),
            &user.access_token,
        )
        .await?;
    ensure!(resp.status() == 403, "expected 403, got {}", resp.status());
    Ok(())
}

/// Sessions scoped to their team in listings.
pub async fn team_session_scoping(ctx: &TestContext) -> Result<()> {
    let (user1, team1) = ctx.setup_user_with_team().await?;
    let (user2, team2) = ctx.setup_user_with_team().await?;

    // Upload to team1
    let session = fixtures::minimal_session();
    ctx.post_json_authed(
        "/sessions",
        &user1.access_token,
        &UploadRequest {
            session,
            team_id: Some(team1.clone()),
            body_url: None,
            linked_session_ids: None,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
        },
    )
    .await?;

    // Upload to team2
    let session = fixtures::minimal_session();
    ctx.post_json_authed(
        "/sessions",
        &user2.access_token,
        &UploadRequest {
            session,
            team_id: Some(team2.clone()),
            body_url: None,
            linked_session_ids: None,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
        },
    )
    .await?;

    // List team1 sessions — should only contain team1 sessions
    let resp = ctx
        .get_authed(&format!("/sessions?team_id={team1}"), &user1.access_token)
        .await?;
    let body: serde_json::Value = resp.json().await?;
    let sessions = body["sessions"].as_array().expect("sessions array");
    ensure!(
        sessions
            .iter()
            .all(|s| s["team_id"].as_str() == Some(&team1)),
        "expected all sessions scoped to team1"
    );
    Ok(())
}
