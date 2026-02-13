use anyhow::{ensure, Result};
use uuid::Uuid;

use opensession_api::{
    AuthRegisterRequest, AuthTokenResponse, ChangePasswordRequest, LoginRequest, LogoutRequest,
    RefreshRequest, UserSettingsResponse,
};

use crate::client::TestContext;

/// POST /api/auth/register → 201, returns valid JWT tokens.
pub async fn register_email(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    ensure!(!user.access_token.is_empty(), "expected access_token");
    ensure!(!user.refresh_token.is_empty(), "expected refresh_token");
    ensure!(user.api_key.starts_with("osk_"), "expected osk_ prefix");
    Ok(())
}

/// Same email → 409.
pub async fn register_duplicate_email(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/register"))
        .json(&AuthRegisterRequest {
            email: user.email.clone(),
            password: "testpass99".into(),
            nickname: format!("dup-{}", &Uuid::new_v4().to_string()[..8]),
        })
        .send()
        .await?;
    ensure!(resp.status() == 409, "expected 409, got {}", resp.status());
    Ok(())
}

/// Same nickname → 409.
pub async fn register_duplicate_nickname(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/register"))
        .json(&AuthRegisterRequest {
            email: format!("dup-{}@e2e.local", Uuid::new_v4()),
            password: "testpass99".into(),
            nickname: user.nickname.clone(),
        })
        .send()
        .await?;
    ensure!(resp.status() == 409, "expected 409, got {}", resp.status());
    Ok(())
}

/// <8 or >12 chars → 400.
pub async fn register_bad_password(ctx: &TestContext) -> Result<()> {
    // Too short
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/register"))
        .json(&AuthRegisterRequest {
            email: format!("short-{}@e2e.local", Uuid::new_v4()),
            password: "short".into(),
            nickname: format!("short-{}", &Uuid::new_v4().to_string()[..8]),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 400,
        "expected 400 for short pw, got {}",
        resp.status()
    );

    // Too long
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/register"))
        .json(&AuthRegisterRequest {
            email: format!("long-{}@e2e.local", Uuid::new_v4()),
            password: "toolongpassword".into(),
            nickname: format!("long-{}", &Uuid::new_v4().to_string()[..8]),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 400,
        "expected 400 for long pw, got {}",
        resp.status()
    );

    Ok(())
}

/// POST /api/auth/login → tokens; wrong password → 401.
pub async fn login(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;

    // Successful login
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/login"))
        .json(&LoginRequest {
            email: user.email.clone(),
            password: user.password.clone(),
        })
        .send()
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());
    let tokens: AuthTokenResponse = resp.json().await?;
    ensure!(!tokens.access_token.is_empty());

    // Wrong password
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/login"))
        .json(&LoginRequest {
            email: user.email.clone(),
            password: "wrongpass99".into(),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 401,
        "expected 401 for wrong pw, got {}",
        resp.status()
    );

    Ok(())
}

/// POST /api/auth/verify with JWT → {user_id, nickname}.
pub async fn verify_jwt(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let resp = ctx.post_authed("/auth/verify", &user.access_token).await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: serde_json::Value = resp.json().await?;
    ensure!(body["user_id"] == user.user_id);
    ensure!(body["nickname"] == user.nickname);
    Ok(())
}

/// POST /api/auth/verify with osk_xxx → {user_id, nickname}.
pub async fn verify_api_key(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let resp = ctx.post_authed("/auth/verify", &user.api_key).await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: serde_json::Value = resp.json().await?;
    ensure!(body["user_id"] == user.user_id);
    Ok(())
}

/// GET /api/auth/me → UserSettingsResponse with all fields.
pub async fn me_endpoint(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let resp = ctx.get_authed("/auth/me", &user.access_token).await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: UserSettingsResponse = resp.json().await?;
    ensure!(body.user_id == user.user_id);
    ensure!(body.nickname == user.nickname);
    ensure!(body.api_key.starts_with("osk_"));
    ensure!(body.email == Some(user.email));
    Ok(())
}

/// POST /api/auth/refresh → new tokens; old refresh invalidated.
pub async fn refresh_token(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;

    // Use refresh token
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/refresh"))
        .json(&RefreshRequest {
            refresh_token: user.refresh_token.clone(),
        })
        .send()
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());
    let new_tokens: AuthTokenResponse = resp.json().await?;
    ensure!(!new_tokens.access_token.is_empty());

    // Old refresh token should be invalidated (rotation)
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/refresh"))
        .json(&RefreshRequest {
            refresh_token: user.refresh_token.clone(),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 401,
        "expected old refresh token invalidated, got {}",
        resp.status()
    );

    Ok(())
}

/// POST /api/auth/logout → ok; refresh now fails 401.
pub async fn logout(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;

    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/logout"))
        .json(&LogoutRequest {
            refresh_token: user.refresh_token.clone(),
        })
        .send()
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    // Refresh token should no longer work
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/refresh"))
        .json(&RefreshRequest {
            refresh_token: user.refresh_token.clone(),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 401,
        "expected 401 after logout, got {}",
        resp.status()
    );

    Ok(())
}

/// PUT /api/auth/password → ok; login with new pw works.
pub async fn change_password(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;
    let new_password = "newpass1234";

    let resp = ctx
        .put_json_authed(
            "/auth/password",
            &user.access_token,
            &ChangePasswordRequest {
                current_password: user.password.clone(),
                new_password: new_password.into(),
            },
        )
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    // Login with new password
    let resp = ctx
        .api
        .reqwest_client()
        .post(ctx.url("/auth/login"))
        .json(&LoginRequest {
            email: user.email.clone(),
            password: new_password.into(),
        })
        .send()
        .await?;
    ensure!(
        resp.status() == 200,
        "expected login with new pw to succeed, got {}",
        resp.status()
    );

    Ok(())
}

/// POST /api/auth/regenerate-key → new key; old key invalid.
pub async fn regenerate_key(ctx: &TestContext) -> Result<()> {
    let user = ctx.register_user().await?;

    let resp = ctx
        .post_authed("/auth/regenerate-key", &user.access_token)
        .await?;
    ensure!(resp.status() == 200, "expected 200, got {}", resp.status());

    let body: serde_json::Value = resp.json().await?;
    let new_key = body["api_key"].as_str().expect("expected api_key string");
    ensure!(new_key.starts_with("osk_"));
    ensure!(new_key != user.api_key, "expected different key");

    // Old key should be invalid
    let resp = ctx.post_authed("/auth/verify", &user.api_key).await?;
    ensure!(
        resp.status() == 401,
        "expected old key invalid, got {}",
        resp.status()
    );

    // New key should work
    let resp = ctx.post_authed("/auth/verify", new_key).await?;
    ensure!(
        resp.status() == 200,
        "expected new key valid, got {}",
        resp.status()
    );

    Ok(())
}
