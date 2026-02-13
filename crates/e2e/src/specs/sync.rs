use anyhow::{ensure, Result};

use opensession_api::{SyncPullResponse, UploadRequest};

use crate::client::TestContext;
use crate::fixtures;

/// GET /api/sync/pull?team_id=X returns uploaded sessions.
pub async fn sync_pull_basic(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    let session = fixtures::minimal_session();
    ctx.post_json_authed(
        "/sessions",
        &user.access_token,
        &UploadRequest {
            session,
            team_id: Some(team_id.clone()),
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

    let resp = ctx
        .get_authed(&format!("/sync/pull?team_id={team_id}"), &user.access_token)
        .await?;
    ensure!(resp.status() == 200);

    let body: SyncPullResponse = resp.json().await?;
    ensure!(!body.sessions.is_empty(), "expected at least 1 session");
    Ok(())
}

/// Cursor-based pagination with `since` param.
pub async fn sync_pull_cursor(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    for _ in 0..3 {
        let session = fixtures::minimal_session();
        ctx.post_json_authed(
            "/sessions",
            &user.access_token,
            &UploadRequest {
                session,
                team_id: Some(team_id.clone()),
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
    }

    // Pull with limit=1
    let resp = ctx
        .get_authed(
            &format!("/sync/pull?team_id={team_id}&limit=1"),
            &user.access_token,
        )
        .await?;
    let page1: SyncPullResponse = resp.json().await?;
    ensure!(page1.sessions.len() == 1);
    ensure!(page1.has_more);
    ensure!(page1.next_cursor.is_some());

    // Pull with cursor (URL-encode because cursor may contain \n)
    let cursor = page1.next_cursor.as_ref().unwrap();
    let encoded_cursor = urlencoding::encode(cursor);
    let resp = ctx
        .get_authed(
            &format!("/sync/pull?team_id={team_id}&limit=1&since={encoded_cursor}"),
            &user.access_token,
        )
        .await?;
    let page2: SyncPullResponse = resp.json().await?;
    ensure!(page2.sessions.len() == 1);
    ensure!(
        page2.sessions[0].id != page1.sessions[0].id,
        "expected different session on page 2"
    );
    Ok(())
}

/// Non-member team → 403.
pub async fn sync_pull_non_member(ctx: &TestContext) -> Result<()> {
    let (_, team_id) = ctx.setup_user_with_team().await?;
    let outsider = ctx.register_user().await?;

    let resp = ctx
        .get_authed(
            &format!("/sync/pull?team_id={team_id}"),
            &outsider.access_token,
        )
        .await?;
    ensure!(resp.status() == 403, "expected 403, got {}", resp.status());
    Ok(())
}
