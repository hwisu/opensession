use anyhow::{ensure, Result};
use uuid::Uuid;

use opensession_api::{UploadRequest, UploadResponse};

use crate::client::TestContext;
use crate::fixtures;

/// POST /api/sessions → 201 {id, url}.
pub async fn upload_session(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;
    let session = fixtures::minimal_session();

    let resp = ctx
        .post_json_authed(
            "/sessions",
            &user.access_token,
            &UploadRequest {
                session,
                team_id: Some(team_id),
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
    ensure!(resp.status() == 201, "expected 201, got {}", resp.status());
    let body: UploadResponse = resp.json().await?;
    ensure!(!body.id.is_empty());
    ensure!(!body.url.is_empty());
    Ok(())
}

/// Non-member upload → 403.
pub async fn upload_requires_membership(ctx: &TestContext) -> Result<()> {
    let (_, team_id) = ctx.setup_user_with_team().await?;
    let outsider = ctx.register_user().await?;
    let session = fixtures::minimal_session();

    let resp = ctx
        .post_json_authed(
            "/sessions",
            &outsider.access_token,
            &UploadRequest {
                session,
                team_id: Some(team_id),
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
    ensure!(resp.status() == 403, "expected 403, got {}", resp.status());
    Ok(())
}

/// Upload 2, GET /api/sessions?team_id=X → both present.
pub async fn list_sessions(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    for _ in 0..2 {
        let session = fixtures::minimal_session();
        let resp = ctx
            .post_json_authed(
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
        ensure!(resp.status() == 201);
    }

    let resp = ctx
        .get_authed(&format!("/sessions?team_id={team_id}"), &user.access_token)
        .await?;
    ensure!(resp.status() == 200);

    let body: serde_json::Value = resp.json().await?;
    let sessions = body["sessions"]
        .as_array()
        .expect("expected sessions array");
    ensure!(sessions.len() >= 2, "expected at least 2 sessions");
    Ok(())
}

/// Upload 3, per_page=1&page=2 → correct page.
pub async fn list_sessions_pagination(ctx: &TestContext) -> Result<()> {
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

    let resp = ctx
        .get_authed(
            &format!("/sessions?team_id={team_id}&per_page=1&page=2"),
            &user.access_token,
        )
        .await?;
    ensure!(resp.status() == 200);

    let body: serde_json::Value = resp.json().await?;
    ensure!(body["page"] == 2);
    ensure!(body["per_page"] == 1);
    let sessions = body["sessions"].as_array().expect("sessions array");
    ensure!(sessions.len() == 1);
    ensure!(body["total"].as_i64().unwrap() >= 3);
    Ok(())
}

/// Upload with unique title, search=UniqueTitle finds it.
pub async fn list_sessions_search(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;
    let unique = format!("UniqueTitle{}", &Uuid::new_v4().to_string()[..8]);

    let session = fixtures::minimal_session_with_title(Some(&unique));
    let resp = ctx
        .post_json_authed(
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
    ensure!(resp.status() == 201);

    let resp = ctx
        .get_authed(
            &format!("/sessions?team_id={team_id}&search={unique}"),
            &user.access_token,
        )
        .await?;
    ensure!(resp.status() == 200);

    let body: serde_json::Value = resp.json().await?;
    let sessions = body["sessions"].as_array().expect("sessions array");
    ensure!(!sessions.is_empty(), "expected search to find session");
    ensure!(
        sessions
            .iter()
            .any(|s| s["title"].as_str() == Some(&unique)),
        "expected matching title"
    );
    Ok(())
}

/// sort=recent ordering correct.
pub async fn list_sessions_sort(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    for _ in 0..2 {
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

    let resp = ctx
        .get_authed(
            &format!("/sessions?team_id={team_id}&sort=recent"),
            &user.access_token,
        )
        .await?;
    ensure!(resp.status() == 200);

    let body: serde_json::Value = resp.json().await?;
    let sessions = body["sessions"].as_array().expect("sessions array");
    ensure!(sessions.len() >= 2);

    // Verify most-recent-first ordering
    if sessions.len() >= 2 {
        let t0 = sessions[0]["uploaded_at"].as_str().unwrap_or("");
        let t1 = sessions[1]["uploaded_at"].as_str().unwrap_or("");
        ensure!(t0 >= t1, "expected recent-first ordering");
    }
    Ok(())
}

/// GET /api/sessions/{id} → correct SessionDetail fields.
pub async fn get_session_detail(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;
    let session = fixtures::minimal_session();

    let resp = ctx
        .post_json_authed(
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
    let upload: UploadResponse = resp.json().await?;

    let resp = ctx
        .get_authed(&format!("/sessions/{}", upload.id), &user.access_token)
        .await?;
    ensure!(resp.status() == 200);

    let detail: serde_json::Value = resp.json().await?;
    ensure!(detail["id"] == upload.id);
    ensure!(detail["team_id"] == team_id);
    ensure!(detail["tool"] == "claude-code");
    Ok(())
}

/// Random UUID → 404.
pub async fn get_session_not_found(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let fake_id = Uuid::new_v4().to_string();

    let resp = ctx
        .get_authed(&format!("/sessions/{fake_id}"), &user.access_token)
        .await?;
    ensure!(resp.status() == 404, "expected 404, got {}", resp.status());
    Ok(())
}

/// GET /api/sessions/{id}/raw → JSONL content.
pub async fn get_session_raw(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;
    let session = fixtures::minimal_session();

    let resp = ctx
        .post_json_authed(
            "/sessions",
            &user.access_token,
            &UploadRequest {
                session,
                team_id: Some(team_id),
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
    let upload: UploadResponse = resp.json().await?;

    let resp = ctx
        .get_authed(&format!("/sessions/{}/raw", upload.id), &user.access_token)
        .await?;
    ensure!(resp.status() == 200);

    let body = resp.text().await?;
    ensure!(!body.is_empty(), "expected non-empty JSONL body");
    ensure!(body.lines().count() >= 1, "expected JSONL content");
    Ok(())
}

/// Upload with linked_session_ids → links in detail.
pub async fn linked_sessions(ctx: &TestContext) -> Result<()> {
    let (user, team_id) = ctx.setup_user_with_team().await?;

    // Upload first session
    let session1 = fixtures::minimal_session();
    let resp = ctx
        .post_json_authed(
            "/sessions",
            &user.access_token,
            &UploadRequest {
                session: session1,
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
    let upload1: UploadResponse = resp.json().await?;

    // Upload second session linked to first
    let session2 = fixtures::minimal_session();
    let resp = ctx
        .post_json_authed(
            "/sessions",
            &user.access_token,
            &UploadRequest {
                session: session2,
                team_id: Some(team_id),
                body_url: None,
                linked_session_ids: Some(vec![upload1.id.clone()]),
                git_remote: None,
                git_branch: None,
                git_commit: None,
                git_repo_name: None,
                pr_number: None,
                pr_url: None,
            },
        )
        .await?;
    let upload2: UploadResponse = resp.json().await?;

    // Check detail of second session has links
    let resp = ctx
        .get_authed(&format!("/sessions/{}", upload2.id), &user.access_token)
        .await?;
    let detail: serde_json::Value = resp.json().await?;
    let links = detail["linked_sessions"]
        .as_array()
        .expect("expected linked_sessions array");
    ensure!(
        links
            .iter()
            .any(|l| l["linked_session_id"].as_str() == Some(&upload1.id)),
        "expected link to first session"
    );
    Ok(())
}
