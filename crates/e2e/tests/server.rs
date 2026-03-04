mod common;

use common::{register_user, test_context_from_env};
use opensession_api::{ChangePasswordRequest, CreateGitCredentialRequest, OkResponse};
use opensession_e2e::client::TestContext;
use serde_json::json;

fn get_ctx() -> TestContext {
    test_context_from_env("OPENSESSION_E2E_SERVER_BASE_URL")
}

macro_rules! e2e_test {
    ($module:ident :: $name:ident) => {
        #[tokio::test]
        async fn $name() {
            let ctx = get_ctx();
            opensession_e2e::specs::$module::$name(&ctx).await.unwrap();
        }
    };
}

opensession_e2e::for_each_spec!(e2e_test);

#[tokio::test]
// @covers server.sessions.repos.list
async fn server_sessions_repos_list() {
    let ctx = get_ctx();
    let response = ctx.get("/sessions/repos").await.expect("request failed");
    assert_eq!(
        response.status().as_u16(),
        200,
        "repos list must be available"
    );

    let body: serde_json::Value = response.json().await.expect("invalid repos response");
    assert!(
        body.get("repos")
            .and_then(|value| value.as_array())
            .is_some(),
        "repos response must include array field `repos`"
    );
}

#[tokio::test]
// @covers server.auth.password.change.success
async fn server_auth_password_change_success() {
    let ctx = get_ctx();
    let user = register_user(&ctx, "server-password", "old-pass-123").await;

    let client = reqwest::Client::new();
    let change_response = client
        .put(ctx.url("/auth/password"))
        .bearer_auth(&user.tokens.access_token)
        .json(&ChangePasswordRequest {
            current_password: user.password.clone(),
            new_password: "new-pass-456".to_string(),
        })
        .send()
        .await
        .expect("password change request failed");

    assert_eq!(
        change_response.status().as_u16(),
        200,
        "password change must succeed"
    );
    let body: OkResponse = change_response
        .json()
        .await
        .expect("invalid change password response");
    assert!(body.ok, "password change response must set ok=true");

    let login_response = client
        .post(ctx.url("/auth/login"))
        .json(&json!({
            "email": user.email,
            "password": "new-pass-456",
        }))
        .send()
        .await
        .expect("login request failed");
    assert_eq!(
        login_response.status().as_u16(),
        200,
        "login must succeed with updated password"
    );
}

#[tokio::test]
// @covers server.auth.git_credentials.crud
async fn server_auth_git_credentials_crud() {
    let ctx = get_ctx();
    let user = register_user(&ctx, "server-git-cred", "test-pass-123").await;

    let client = reqwest::Client::new();
    let create_response = client
        .post(ctx.url("/auth/git-credentials"))
        .bearer_auth(&user.tokens.access_token)
        .json(&CreateGitCredentialRequest {
            label: "GitLab Internal".to_string(),
            host: "gitlab.internal.example.com".to_string(),
            path_prefix: Some("group/subgroup".to_string()),
            header_name: "Authorization".to_string(),
            header_value: "Bearer secret-token".to_string(),
        })
        .send()
        .await
        .expect("create credential request failed");

    if create_response.status().as_u16() == 500 {
        let body = create_response
            .text()
            .await
            .expect("read credential create failure");
        if body.contains("credential encryption is not configured") {
            eprintln!(
                "skipping git credential CRUD assertions: credential encryption is not configured"
            );
            return;
        }
        panic!("unexpected credential create failure: {body}");
    }

    assert_eq!(
        create_response.status().as_u16(),
        201,
        "credential creation must succeed",
    );
    let created: serde_json::Value = create_response
        .json()
        .await
        .expect("invalid credential create response");
    let credential_id = created
        .get("id")
        .and_then(|value| value.as_str())
        .expect("credential id missing")
        .to_string();

    let list_response = client
        .get(ctx.url("/auth/git-credentials"))
        .bearer_auth(&user.tokens.access_token)
        .send()
        .await
        .expect("list credentials request failed");
    assert_eq!(
        list_response.status().as_u16(),
        200,
        "credential list must succeed"
    );
    let list_body: serde_json::Value = list_response
        .json()
        .await
        .expect("invalid list credentials response");
    let listed = list_body
        .get("credentials")
        .and_then(|value| value.as_array())
        .expect("credentials array missing")
        .iter()
        .any(|row| row.get("id").and_then(|value| value.as_str()) == Some(&credential_id));
    assert!(
        listed,
        "created credential must be present in list response"
    );

    let delete_response = client
        .delete(ctx.url(&format!("/auth/git-credentials/{credential_id}")))
        .bearer_auth(&user.tokens.access_token)
        .send()
        .await
        .expect("delete credential request failed");
    assert_eq!(
        delete_response.status().as_u16(),
        200,
        "credential delete must succeed"
    );
    let delete_body: OkResponse = delete_response
        .json()
        .await
        .expect("invalid delete credential response");
    assert!(delete_body.ok, "delete response must set ok=true");
}

#[tokio::test]
// @covers server.admin.delete_session.authz
async fn server_admin_delete_session_authz() {
    let ctx = get_ctx();
    let response = reqwest::Client::new()
        .delete(ctx.url(&format!(
            "/admin/sessions/{}",
            uuid::Uuid::new_v4().simple()
        )))
        .send()
        .await
        .expect("delete session request failed");

    assert_eq!(
        response.status().as_u16(),
        401,
        "admin delete endpoint must reject unauthenticated request"
    );
}

#[tokio::test]
async fn removed_team_and_sync_endpoints_are_unavailable() {
    let ctx = get_ctx();
    let client = reqwest::Client::new();
    for path in ["/teams", "/invitations", "/sync/pull"] {
        let response = client
            .post(ctx.url(path))
            .json(&json!({}))
            .send()
            .await
            .expect("request failed");
        let status = response.status().as_u16();
        assert!(
            (400..500).contains(&status),
            "expected removed endpoint to be unavailable for {path}, got {status}",
        );
    }
}
