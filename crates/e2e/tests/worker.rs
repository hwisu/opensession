use opensession_e2e::client::TestContext;
use tokio::sync::OnceCell;

static CTX: OnceCell<TestContext> = OnceCell::const_new();

async fn get_ctx() -> &'static TestContext {
    CTX.get_or_init(|| async {
        let base_url = std::env::var("OPENSESSION_BASE_URL")
            .unwrap_or_else(|_| "https://opensession.io".into());
        let ctx = TestContext::new(base_url);

        let email = std::env::var("E2E_ADMIN_EMAIL").expect("E2E_ADMIN_EMAIL required");
        let password = std::env::var("E2E_ADMIN_PASSWORD").expect("E2E_ADMIN_PASSWORD required");
        ctx.setup_admin_with_credentials(&email, &password)
            .await
            .expect("admin login failed");
        ctx
    })
    .await
}

macro_rules! e2e_test {
    ($module:ident :: $name:ident) => {
        #[tokio::test]
        async fn $name() {
            let ctx = get_ctx().await;
            opensession_e2e::specs::$module::$name(ctx).await.unwrap();
        }
    };
}

opensession_e2e::for_each_spec!(e2e_test);
