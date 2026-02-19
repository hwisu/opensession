use opensession_api::UploadRequest;
use opensession_e2e::client::TestContext;
use serde_json::json;

async fn get_ctx() -> TestContext {
    let base_url = std::env::var("BASE_URL")
        .or_else(|_| std::env::var("OPENSESSION_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:3000".into());
    TestContext::new(base_url)
}

macro_rules! e2e_test {
    ($module:ident :: $name:ident) => {
        #[tokio::test]
        async fn $name() {
            let ctx = get_ctx().await;
            opensession_e2e::specs::$module::$name(&ctx).await.unwrap();
        }
    };
}

opensession_e2e::for_each_spec!(e2e_test);

async fn register_access_token(ctx: &TestContext) -> String {
    let client = reqwest::Client::new();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let resp = client
        .post(ctx.url("/auth/register"))
        .json(&json!({
            "email": format!("e2e-{suffix}@local.test"),
            "password": "testpass99",
            "nickname": format!("e2e-{suffix}"),
        }))
        .send()
        .await
        .expect("register request failed");

    assert_eq!(
        resp.status().as_u16(),
        201,
        "register must succeed for authenticated upload test"
    );
    let body: serde_json::Value = resp.json().await.expect("invalid auth register response");
    body["access_token"]
        .as_str()
        .expect("missing access_token")
        .to_string()
}

#[tokio::test]
async fn upload_requires_auth_on_server_profile() {
    let ctx = get_ctx().await;
    let session = opensession_e2e::fixtures::minimal_session();
    let resp = ctx
        .post_json(
            "/sessions",
            &UploadRequest {
                session,
                body_url: None,
                linked_session_ids: None,
                git_remote: None,
                git_branch: None,
                git_commit: None,
                git_repo_name: None,
                pr_number: None,
                pr_url: None,
                score_plugin: None,
            },
        )
        .await
        .expect("request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "server profile must require authentication for uploads"
    );
}

#[tokio::test]
async fn upload_route_accepts_authenticated_upload_without_team_id() {
    let ctx = get_ctx().await;
    let access_token = register_access_token(&ctx).await;
    let session = opensession_e2e::fixtures::minimal_session();

    let client = reqwest::Client::new();
    let resp = client
        .post(ctx.url("/sessions"))
        .bearer_auth(access_token)
        .json(&UploadRequest {
            session,
            body_url: None,
            linked_session_ids: None,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            score_plugin: None,
        })
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status().as_u16(),
        201,
        "server profile must accept authenticated session upload without team_id"
    );
}

#[tokio::test]
async fn removed_team_and_sync_endpoints_return_not_found() {
    let ctx = get_ctx().await;
    for path in ["/teams", "/invitations", "/sync/pull"] {
        let resp = ctx.get(path).await.expect("request failed");
        assert_eq!(
            resp.status().as_u16(),
            404,
            "expected 404 for removed endpoint {path}"
        );
    }
}
