//! Shared business logic — framework-agnostic pure functions.
//!
//! Both the Axum server and Cloudflare Worker call these functions,
//! keeping route handlers as thin adapters.

use crate::{AuthTokenResponse, ServiceError};

// ─── Validation ─────────────────────────────────────────────────────────────

/// Validate and normalize an email address. Returns the lowercased, trimmed email.
pub fn validate_email(email: &str) -> Result<String, ServiceError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') || email.len() > 254 {
        return Err(ServiceError::BadRequest("invalid email address".into()));
    }
    Ok(email)
}

const MIN_PASSWORD_CHARS: usize = 12;
const MAX_PASSWORD_CHARS: usize = 128;

/// Validate a password (12-128 characters).
pub fn validate_password(password: &str) -> Result<(), ServiceError> {
    if password.len() < MIN_PASSWORD_CHARS {
        return Err(ServiceError::BadRequest(
            "password must be at least 12 characters".into(),
        ));
    }
    if password.len() > MAX_PASSWORD_CHARS {
        return Err(ServiceError::BadRequest(
            "password must be at most 128 characters".into(),
        ));
    }
    Ok(())
}

/// Validate and normalize a user nickname. Returns the trimmed nickname.
pub fn validate_nickname(nickname: &str) -> Result<String, ServiceError> {
    let trimmed = nickname.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(ServiceError::BadRequest(
            "nickname must be 1-64 characters".into(),
        ));
    }
    Ok(trimmed)
}

// ─── API Key Generation ─────────────────────────────────────────────────────

/// Grace period for old API keys after issuing a new key.
pub const API_KEY_GRACE_DAYS: i64 = 7;

/// Generate a new API key with the `osk_` prefix.
pub fn generate_api_key() -> String {
    format!("osk_{}", uuid::Uuid::new_v4().simple())
}

/// Hash an API key for persistent storage and lookup.
pub fn hash_api_key(api_key: &str) -> String {
    crate::crypto::hash_token(api_key)
}

/// Prefix used for operator-facing key previews.
pub fn key_prefix(api_key: &str) -> String {
    api_key.chars().take(12).collect()
}

/// Compute grace deadline in SQLite datetime format.
pub fn grace_until_sqlite(now_unix: u64) -> Result<String, ServiceError> {
    let base = chrono::DateTime::from_timestamp(now_unix as i64, 0)
        .ok_or_else(|| ServiceError::Internal("invalid timestamp".into()))?;
    Ok((base + chrono::Duration::days(API_KEY_GRACE_DAYS))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string())
}

// ─── Auth Token Resolution ──────────────────────────────────────────────────

/// Result of resolving an auth token string.
pub enum AuthToken {
    /// JWT was valid — contains the extracted user_id.
    Jwt(String),
    /// Token is an API key (`osk_` prefix) — caller must look up in DB.
    ApiKey(String),
}

/// Resolve an auth token string into either a verified JWT user_id or an API key.
///
/// This centralizes the JWT-vs-API-key branching logic shared by both backends.
/// Each backend only needs to extract the token string from headers/cookies and
/// then call this function.
pub fn resolve_auth_token(
    token: &str,
    jwt_secret: &str,
    now: u64,
) -> Result<AuthToken, ServiceError> {
    if token.starts_with("osk_") {
        return Ok(AuthToken::ApiKey(token.to_string()));
    }

    if jwt_secret.is_empty() {
        return Err(ServiceError::Unauthorized(
            "JWT authentication not configured".into(),
        ));
    }

    let user_id = crate::crypto::verify_jwt(token, jwt_secret, now)?;
    Ok(AuthToken::Jwt(user_id))
}

// ─── Token Bundle ───────────────────────────────────────────────────────────

/// Pre-computed token bundle returned by [`prepare_token_bundle`].
///
/// Contains everything needed to insert a refresh token and return the auth
/// response. The caller only needs to perform the DB INSERT.
pub struct TokenBundle {
    /// JWT access token.
    pub access_token: String,
    /// Raw refresh token (sent to the client).
    pub refresh_token: String,
    /// SHA-256 hash of the refresh token (stored in DB).
    pub token_hash: String,
    /// UUID primary key for the refresh_tokens row.
    pub token_id: String,
    /// `datetime` string for the refresh token expiry (DB column value).
    pub expires_at: String,
    /// Ready-to-return API response.
    pub response: AuthTokenResponse,
}

/// Build a [`TokenBundle`] containing a JWT, refresh token, and the auth response.
///
/// This is the pure-computation part of `issue_tokens`. Each backend only needs
/// to insert the refresh token row into its database.
pub fn prepare_token_bundle(
    jwt_secret: &str,
    user_id: &str,
    nickname: &str,
    now_unix: u64,
) -> Result<TokenBundle, ServiceError> {
    use crate::crypto;

    let access_token = crypto::sign_jwt(user_id, jwt_secret, now_unix);
    let refresh_token = crypto::generate_token()?;
    let token_hash = crypto::hash_token(&refresh_token);
    let token_id = uuid::Uuid::new_v4().to_string();

    let base = chrono::DateTime::from_timestamp(now_unix as i64, 0)
        .ok_or_else(|| ServiceError::Internal("invalid timestamp".into()))?;
    let expires_at = base
        .checked_add_signed(chrono::Duration::seconds(
            crypto::REFRESH_EXPIRY_SECS as i64,
        ))
        .ok_or_else(|| ServiceError::Internal("timestamp overflow".into()))?
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let response = AuthTokenResponse {
        access_token: access_token.clone(),
        refresh_token: refresh_token.clone(),
        expires_in: crypto::JWT_EXPIRY_SECS,
        user_id: user_id.to_string(),
        nickname: nickname.to_string(),
    };

    Ok(TokenBundle {
        access_token,
        refresh_token,
        token_hash,
        token_id,
        expires_at,
        response,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // @coversfn service.validate_email success
    // @coversfn service.validate_email error
    #[test]
    fn test_validate_email() {
        assert_eq!(
            validate_email("  USER@Example.Com ").expect("email should normalize"),
            "user@example.com"
        );
        assert!(validate_email("").is_err());
        assert!(validate_email("not-an-email").is_err());
    }

    // @coversfn service.validate_password success
    // @coversfn service.validate_password error
    #[test]
    fn test_validate_password_bounds() {
        assert!(validate_password("x".repeat(11).as_str()).is_err());
        assert!(validate_password("x".repeat(12).as_str()).is_ok());
        assert!(validate_password("x".repeat(128).as_str()).is_ok());
        assert!(validate_password("x".repeat(129).as_str()).is_err());
    }

    // @coversfn service.validate_nickname success
    // @coversfn service.validate_nickname error
    #[test]
    fn test_validate_nickname() {
        assert!(validate_nickname("alice").is_ok());
        assert_eq!(validate_nickname("  bob  ").unwrap(), "bob");
        assert!(validate_nickname("").is_err());
        assert!(validate_nickname("   ").is_err());
        assert!(validate_nickname(&"x".repeat(65)).is_err());
        assert!(validate_nickname(&"x".repeat(64)).is_ok());
    }

    // @coversfn service.generate_api_key success
    #[test]
    fn test_generate_api_key_prefix_and_length() {
        let key = generate_api_key();
        assert!(key.starts_with("osk_"));
        assert_eq!(key.len(), 36);
    }

    // @coversfn service.hash_api_key success
    #[test]
    fn test_hash_api_key_is_stable() {
        let raw = "osk_1234567890abcdef";
        let hash_a = hash_api_key(raw);
        let hash_b = hash_api_key(raw);
        assert_eq!(hash_a, hash_b);
        assert_ne!(hash_a, raw);
        assert_eq!(hash_a.len(), 64);
    }

    // @coversfn service.key_prefix success
    #[test]
    fn test_key_prefix_truncates_to_twelve_chars() {
        assert_eq!(key_prefix("osk_abcdefghijklmnopqrstuvwxyz"), "osk_abcdefgh");
        assert_eq!(key_prefix("short"), "short");
    }

    // @coversfn service.grace_until_sqlite success
    // @coversfn service.grace_until_sqlite error
    #[test]
    fn test_grace_until_sqlite() {
        assert_eq!(
            grace_until_sqlite(0).expect("epoch should be valid"),
            "1970-01-08 00:00:00"
        );
        let err = grace_until_sqlite(i64::MIN as u64).expect_err("out-of-range ts must fail");
        assert!(matches!(err, ServiceError::Internal(_)));
    }

    // @coversfn service.resolve_auth_token success
    #[test]
    fn test_resolve_auth_token_success_paths() {
        match resolve_auth_token("osk_live_token", "", 0).expect("api keys should bypass jwt") {
            AuthToken::ApiKey(token) => assert_eq!(token, "osk_live_token"),
            AuthToken::Jwt(_) => panic!("api key should not resolve as jwt"),
        }

        let now = 1_700_000_000_u64;
        let secret = "jwt-secret";
        let jwt = crate::crypto::sign_jwt("user-1", secret, now);
        match resolve_auth_token(&jwt, secret, now).expect("valid jwt should resolve") {
            AuthToken::Jwt(user_id) => assert_eq!(user_id, "user-1"),
            AuthToken::ApiKey(_) => panic!("jwt should not resolve as api key"),
        }
    }

    // @coversfn service.resolve_auth_token error
    #[test]
    fn test_resolve_auth_token_error_paths() {
        let missing_secret = match resolve_auth_token("definitely-not-an-api-key", "", 0) {
            Ok(_) => panic!("expected missing jwt secret to fail"),
            Err(err) => err,
        };
        assert!(matches!(missing_secret, ServiceError::Unauthorized(_)));

        let invalid_jwt = match resolve_auth_token("definitely-not-a-jwt", "jwt-secret", 0) {
            Ok(_) => panic!("expected invalid jwt to fail"),
            Err(err) => err,
        };
        assert!(matches!(invalid_jwt, ServiceError::Unauthorized(_)));
    }

    // @coversfn service.prepare_token_bundle success
    #[test]
    fn test_prepare_token_bundle_success() {
        let now = 1_700_000_000_u64;
        let bundle = prepare_token_bundle("jwt-secret", "user-2", "alice", now)
            .expect("token bundle should be created");
        assert_eq!(bundle.response.user_id, "user-2");
        assert_eq!(bundle.response.nickname, "alice");
        assert_eq!(bundle.response.access_token, bundle.access_token);
        assert_eq!(bundle.response.refresh_token, bundle.refresh_token);
        assert_eq!(
            bundle.token_hash,
            crate::crypto::hash_token(&bundle.refresh_token)
        );
        assert!(uuid::Uuid::parse_str(&bundle.token_id).is_ok());
        assert_eq!(bundle.expires_at.len(), 19);
    }

    // @coversfn service.prepare_token_bundle error
    #[test]
    fn test_prepare_token_bundle_rejects_invalid_timestamp() {
        let err = match prepare_token_bundle("jwt-secret", "user-2", "alice", i64::MIN as u64) {
            Ok(_) => panic!("expected out-of-range timestamp to fail"),
            Err(err) => err,
        };
        assert!(matches!(err, ServiceError::Internal(_)));
    }
}
