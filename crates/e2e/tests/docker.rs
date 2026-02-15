use opensession_e2e::client::TestContext;
async fn get_ctx() -> TestContext {
    let base_url = std::env::var("BASE_URL")
        .or_else(|_| std::env::var("OPENSESSION_BASE_URL"))
        .unwrap_or_else(|_| "http://localhost:3000".into());
    let ctx = TestContext::new(base_url);
    ctx.setup_admin().await.expect("admin setup failed");
    ctx
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
opensession_e2e::for_each_docker_only_spec!(e2e_test);

#[tokio::test]
async fn team_api_enabled_on_docker_profile() {
    let ctx = get_ctx().await;
    let admin = ctx.admin().expect("admin not initialized");
    let resp = ctx
        .get_authed("/teams", &admin.access_token)
        .await
        .expect("request failed");

    assert_ne!(
        resp.status().as_u16(),
        404,
        "docker profile must expose /api/teams"
    );
}
