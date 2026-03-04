mod common;

use base64::Engine;
use common::{register_user, test_context_from_env};
use opensession_api::{ParsePreviewRequest, ParseSource};
use opensession_e2e::client::TestContext;
use serde_json::json;

fn get_ctx() -> TestContext {
    test_context_from_env("OPENSESSION_E2E_WORKER_BASE_URL")
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
async fn auth_providers_endpoint_is_available_in_worker() {
    let ctx = get_ctx();
    let response = ctx.get("/auth/providers").await.expect("request failed");
    assert_eq!(
        response.status().as_u16(),
        200,
        "worker profile must expose /api/auth/providers",
    );
    let body: serde_json::Value = response.json().await.expect("invalid providers response");
    assert!(
        body.get("email_password")
            .and_then(|value| value.as_bool())
            .is_some(),
        "providers response must include boolean email_password",
    );
}

#[tokio::test]
async fn worker_auth_register_login_me_refresh_logout_flow() {
    let ctx = get_ctx();
    let user = register_user(&ctx, "worker-auth-flow", "testpass-12345").await;
    let access_token = user.tokens.access_token;
    let refresh_token = user.tokens.refresh_token;

    let client = reqwest::Client::new();
    let me_response = client
        .get(ctx.url("/auth/me"))
        .bearer_auth(&access_token)
        .send()
        .await
        .expect("me request failed");
    assert_eq!(
        me_response.status().as_u16(),
        200,
        "me must succeed with worker access token",
    );
    let me_body: serde_json::Value = me_response.json().await.expect("invalid me response");
    assert!(
        me_body.get("api_key").is_none(),
        "me response must not include api_key",
    );

    let issue_key_response = client
        .post(ctx.url("/auth/api-keys/issue"))
        .bearer_auth(&access_token)
        .send()
        .await
        .expect("issue api key request failed");
    assert_eq!(
        issue_key_response.status().as_u16(),
        200,
        "api key issue endpoint must succeed in worker profile",
    );
    let issue_key_body: serde_json::Value = issue_key_response
        .json()
        .await
        .expect("invalid issue api key response");
    let issued_api_key = issue_key_body
        .get("api_key")
        .and_then(|value| value.as_str())
        .expect("missing api_key in issue response");
    assert!(
        issued_api_key.starts_with("osk_"),
        "issued api key must have osk_ prefix",
    );

    let refresh_response = client
        .post(ctx.url("/auth/refresh"))
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .expect("refresh request failed");
    assert_eq!(
        refresh_response.status().as_u16(),
        200,
        "refresh must succeed in worker profile",
    );
    let refresh_body: serde_json::Value = refresh_response
        .json()
        .await
        .expect("invalid refresh response");
    let rotated_refresh = refresh_body
        .get("refresh_token")
        .and_then(|value| value.as_str())
        .expect("missing rotated refresh token")
        .to_string();

    let logout_response = client
        .post(ctx.url("/auth/logout"))
        .bearer_auth(&access_token)
        .json(&json!({ "refresh_token": rotated_refresh }))
        .send()
        .await
        .expect("logout request failed");
    assert_eq!(
        logout_response.status().as_u16(),
        200,
        "logout must succeed in worker profile",
    );
}

#[tokio::test]
async fn worker_auth_oauth_redirect_callback_routes_are_exposed() {
    let ctx = get_ctx();
    let providers_response = ctx.get("/auth/providers").await.expect("request failed");
    assert_eq!(providers_response.status().as_u16(), 200);
    let providers: serde_json::Value = providers_response
        .json()
        .await
        .expect("invalid providers response");

    let oauth = providers
        .get("oauth")
        .and_then(|value| value.as_array())
        .expect("oauth providers must be an array");
    if oauth.is_empty() {
        // Provider secrets are not configured in this environment.
        return;
    }

    let provider_id = oauth[0]
        .get("id")
        .and_then(|value| value.as_str())
        .expect("oauth provider missing id")
        .to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("reqwest client");

    let redirect_response = client
        .get(ctx.url(&format!("/auth/oauth/{provider_id}")))
        .send()
        .await
        .expect("oauth redirect request failed");
    assert_eq!(
        redirect_response.status().as_u16(),
        302,
        "oauth redirect must return 302 when provider is enabled",
    );
    let location = redirect_response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("oauth redirect response must include Location header");
    let location_url = reqwest::Url::parse(location).expect("oauth redirect location");
    let state = location_url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then_some(value.to_string()))
        .expect("oauth redirect must include state query param");

    let callback_response = client
        .get(ctx.url(&format!("/auth/oauth/{provider_id}/callback?state={state}")))
        .send()
        .await
        .expect("oauth callback request failed");
    assert_eq!(
        callback_response.status().as_u16(),
        400,
        "oauth callback route must reject missing code with 400",
    );
}

#[tokio::test]
async fn worker_parse_preview_inline_success() {
    let ctx = get_ctx();
    let session = opensession_e2e::fixtures::minimal_session();
    let source_body = session.to_jsonl().expect("serialize fixture session");
    let encoded = base64::engine::general_purpose::STANDARD.encode(source_body);

    let response = reqwest::Client::new()
        .post(ctx.url("/parse/preview"))
        .json(&ParsePreviewRequest {
            source: ParseSource::Inline {
                filename: "inline.hail.jsonl".to_string(),
                content_base64: encoded,
            },
            parser_hint: None,
        })
        .send()
        .await
        .expect("parse preview request failed");

    assert_eq!(
        response.status().as_u16(),
        200,
        "inline parse preview must succeed",
    );
    let body: serde_json::Value = response
        .json()
        .await
        .expect("invalid parse preview response");
    assert_eq!(
        body.get("parser_used").and_then(|value| value.as_str()),
        Some("hail")
    );
    assert!(
        body.get("session")
            .and_then(|value| value.get("session_id"))
            .and_then(|value| value.as_str())
            .is_some(),
        "parse preview response must include session payload",
    );
}

#[tokio::test]
async fn worker_parse_preview_git_credential_required() {
    let ctx = get_ctx();
    let user = register_user(&ctx, "worker-git-credential", "testpass-12345").await;
    let access_token = user.tokens.access_token;

    let response = reqwest::Client::new()
        .post(ctx.url("/parse/preview"))
        .bearer_auth(access_token)
        .json(&ParsePreviewRequest {
            source: ParseSource::Git {
                remote: "https://huggingface.co/private/repo".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            parser_hint: None,
        })
        .send()
        .await
        .expect("git parse preview request failed");

    if response.status().as_u16() == 422 {
        let body: serde_json::Value = response
            .json()
            .await
            .expect("invalid git parse preview error response");
        if body.get("code").and_then(|value| value.as_str()) == Some("fetch_failed") {
            eprintln!(
                "skipping credential-required assertion: worker runtime could not initialize outbound fetch for the remote source"
            );
            return;
        }
        panic!("unexpected 422 parse preview error: {body}");
    }

    assert_eq!(
        response.status().as_u16(),
        401,
        "worker should request credentials for denied authenticated git fetch",
    );
    let body: serde_json::Value = response
        .json()
        .await
        .expect("invalid git parse preview error response");
    assert_eq!(
        body.get("code").and_then(|value| value.as_str()),
        Some("missing_git_credential"),
    );
}
