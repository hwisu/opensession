use opensession_api::UploadRequest;
use opensession_e2e::client::TestContext;
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

#[tokio::test]
async fn upload_route_is_available_on_server_profile() {
    let ctx = get_ctx().await;
    let session = opensession_e2e::fixtures::minimal_session();
    let resp = ctx
        .post_json(
            "/sessions",
            &UploadRequest {
                session,
                team_id: Some("local".to_string()),
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
        201,
        "server profile must accept session upload"
    );
}
