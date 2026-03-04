use opensession_api::AuthTokenResponse;
use opensession_e2e::client::TestContext;
use reqwest::Url;
use serde_json::json;

const ENV_ALLOW_REMOTE: &str = "OPENSESSION_E2E_ALLOW_REMOTE";

#[allow(dead_code)]
pub struct RegisteredUser {
    pub email: String,
    pub password: String,
    pub tokens: AuthTokenResponse,
}

pub fn test_context_from_env(base_url_env: &str) -> TestContext {
    let base_url = std::env::var(base_url_env).unwrap_or_else(|_| {
        panic!(
            "missing required env var `{base_url_env}`. Set explicit local target URL for this E2E run."
        )
    });
    enforce_base_url_policy(base_url_env, &base_url);
    TestContext::new(base_url)
}

pub async fn register_user(ctx: &TestContext, prefix: &str, password: &str) -> RegisteredUser {
    let client = reqwest::Client::new();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let email = format!("{prefix}-{suffix}@local.test");
    let nickname = format!("{prefix}-{suffix}");
    let response = client
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
        response.status().as_u16(),
        201,
        "register must succeed for test user bootstrap",
    );
    let tokens = response
        .json::<AuthTokenResponse>()
        .await
        .expect("invalid auth register response");
    RegisteredUser {
        email,
        password: password.to_string(),
        tokens,
    }
}

fn enforce_base_url_policy(env_name: &str, base_url: &str) {
    let parsed = Url::parse(base_url).unwrap_or_else(|err| {
        panic!("invalid `{env_name}` URL `{base_url}`: {err}");
    });
    let host = parsed
        .host_str()
        .unwrap_or_else(|| panic!("`{env_name}` must include a valid host: `{base_url}`"));

    if !allow_remote_targets() && !is_local_host(host) {
        panic!(
            "remote E2E target blocked by default: `{env_name}` is `{base_url}`. \
Set `{ENV_ALLOW_REMOTE}=1` only when you intentionally run against remote infrastructure."
        );
    }
}

fn allow_remote_targets() -> bool {
    std::env::var(ENV_ALLOW_REMOTE)
        .ok()
        .map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" | "" => false,
            _ => false,
        })
        .unwrap_or(false)
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}
