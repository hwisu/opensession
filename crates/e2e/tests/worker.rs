use opensession_api::UploadRequest;
use opensession_e2e::client::TestContext;
use serde_json::json;

async fn get_ctx() -> TestContext {
    let base_url = std::env::var("BASE_URL")
        .or_else(|_| std::env::var("OPENSESSION_BASE_URL"))
        .unwrap_or_else(|_| "https://opensession.io".into());
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
async fn upload_route_is_disabled_in_worker_profile() {
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

    let status = resp.status().as_u16();
    assert!(
        status == 404 || status == 405,
        "worker profile must reject upload route, got status {status}"
    );
}

#[tokio::test]
async fn auth_providers_endpoint_is_available_in_worker() {
    let ctx = get_ctx().await;
    let resp = ctx.get("/auth/providers").await.expect("request failed");
    assert_eq!(
        resp.status().as_u16(),
        200,
        "worker profile must expose /api/auth/providers"
    );
    let body: serde_json::Value = resp.json().await.expect("invalid providers response");
    assert!(
        body.get("email_password")
            .and_then(|v| v.as_bool())
            .is_some(),
        "providers response must include boolean email_password"
    );
}

#[tokio::test]
async fn worker_auth_register_login_me_refresh_logout_flow() {
    let ctx = get_ctx().await;
    let client = reqwest::Client::new();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let email = format!("worker-e2e-{suffix}@local.test");
    let nickname = format!("worker-e2e-{suffix}");
    let password = "testpass99";

    let register_resp = client
        .post(ctx.url("/auth/register"))
        .json(&json!({
            "email": email,
            "password": password,
            "nickname": nickname,
        }))
        .send()
        .await
        .expect("register request failed");
    assert_eq!(
        register_resp.status().as_u16(),
        201,
        "register must succeed in worker profile"
    );
    let register_body: serde_json::Value = register_resp
        .json()
        .await
        .expect("invalid register response");
    let access_token = register_body["access_token"]
        .as_str()
        .expect("missing access_token")
        .to_string();
    let refresh_token = register_body["refresh_token"]
        .as_str()
        .expect("missing refresh_token")
        .to_string();

    let me_resp = client
        .get(ctx.url("/auth/me"))
        .bearer_auth(&access_token)
        .send()
        .await
        .expect("me request failed");
    assert_eq!(
        me_resp.status().as_u16(),
        200,
        "me must succeed with worker access token"
    );
    let me_body: serde_json::Value = me_resp.json().await.expect("invalid me response");
    assert!(
        me_body.get("api_key").and_then(|v| v.as_str()).is_some(),
        "me response must include api_key"
    );

    let refresh_resp = client
        .post(ctx.url("/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .expect("refresh request failed");
    assert_eq!(
        refresh_resp.status().as_u16(),
        200,
        "refresh must succeed in worker profile"
    );
    let refresh_body: serde_json::Value =
        refresh_resp.json().await.expect("invalid refresh response");
    let rotated_refresh = refresh_body["refresh_token"]
        .as_str()
        .expect("missing rotated refresh token")
        .to_string();

    let logout_resp = client
        .post(ctx.url("/auth/logout"))
        .bearer_auth(&access_token)
        .json(&json!({ "refresh_token": rotated_refresh }))
        .send()
        .await
        .expect("logout request failed");
    assert_eq!(
        logout_resp.status().as_u16(),
        200,
        "logout must succeed in worker profile"
    );
}

#[tokio::test]
async fn worker_oauth_redirect_returns_302_when_provider_enabled() {
    let ctx = get_ctx().await;
    let providers_resp = ctx.get("/auth/providers").await.expect("request failed");
    assert_eq!(providers_resp.status().as_u16(), 200);
    let providers: serde_json::Value = providers_resp
        .json()
        .await
        .expect("invalid providers response");

    let oauth = providers["oauth"]
        .as_array()
        .expect("oauth providers must be an array");
    if oauth.is_empty() {
        // Provider secrets are not configured in this environment.
        return;
    }
    let provider_id = oauth[0]["id"]
        .as_str()
        .expect("oauth provider missing id")
        .to_string();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client");
    let redirect_resp = client
        .get(ctx.url(&format!("/auth/oauth/{provider_id}")))
        .send()
        .await
        .expect("oauth redirect request failed");

    assert_eq!(
        redirect_resp.status().as_u16(),
        302,
        "oauth redirect must return 302 when provider is enabled"
    );
    assert!(
        redirect_resp.headers().get("location").is_some(),
        "oauth redirect response must include Location header"
    );
}
