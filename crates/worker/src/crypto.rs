//! Re-export shared crypto from opensession-api.
//!
//! The actual implementation lives in `opensession_api::crypto`.
//! This module provides a `verify_jwt` wrapper that converts `ServiceError`
//! to `worker::Error` for backward compatibility with Worker routes.

pub use opensession_api::crypto::{hash_password, hash_token, verify_password};

/// Generate a secure random token, returning `worker::Result` for Worker compatibility.
pub fn generate_token() -> worker::Result<String> {
    opensession_api::crypto::generate_token()
        .map_err(|e| worker::Error::from(e.message().to_string()))
}

/// Verify JWT, returning `worker::Result` for Worker compatibility.
pub fn verify_jwt(token: &str, secret: &str, now_unix: u64) -> worker::Result<String> {
    opensession_api::crypto::verify_jwt(token, secret, now_unix)
        .map_err(|e| worker::Error::from(e.message().to_string()))
}
