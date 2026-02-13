//! Cryptographic helpers for authentication.
//!
//! - PBKDF2-SHA256 password hashing (600k iterations)
//! - HMAC-SHA256 JWT signing/verification
//!
//! Uses pure Rust crates (wasm-compatible, no WebCrypto interop needed).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

use crate::ServiceError;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

// ── Password hashing ────────────────────────────────────────────────────────

/// Hash a password with PBKDF2-SHA256. Returns `(hash_hex, salt_hex)`.
pub fn hash_password(password: &str) -> Result<(String, String), ServiceError> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt)
        .map_err(|e| ServiceError::Internal(format!("RNG failure: {e}")))?;

    let mut hash = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut hash);

    Ok((hex::encode(hash), hex::encode(salt)))
}

/// Verify a password against a stored hash and salt (both hex-encoded).
pub fn verify_password(password: &str, hash_hex: &str, salt_hex: &str) -> bool {
    let Ok(salt) = hex::decode(salt_hex) else {
        return false;
    };
    let Ok(expected) = hex::decode(hash_hex) else {
        return false;
    };

    let mut hash = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut hash);

    // Constant-time comparison
    hash.len() == expected.len() && hash.iter().zip(expected.iter()).all(|(a, b)| a == b)
}

// ── JWT (HMAC-SHA256) ───────────────────────────────────────────────────────

/// JWT header (always HS256).
const JWT_HEADER: &str = r#"{"alg":"HS256","typ":"JWT"}"#;

/// JWT expiry: 1 hour in seconds.
pub const JWT_EXPIRY_SECS: u64 = 3600;

/// Refresh token expiry: 7 days in seconds.
pub const REFRESH_EXPIRY_SECS: u64 = 7 * 24 * 3600;

/// Sign a JWT for the given user. Returns the encoded JWT string.
pub fn sign_jwt(user_id: &str, secret: &str, now_unix: u64) -> String {
    let header_b64 = URL_SAFE_NO_PAD.encode(JWT_HEADER.as_bytes());

    let payload = format!(
        r#"{{"sub":"{}","iat":{},"exp":{}}}"#,
        user_id,
        now_unix,
        now_unix + JWT_EXPIRY_SECS,
    );
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());

    let signing_input = format!("{header_b64}.{payload_b64}");
    let signature = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature);

    format!("{signing_input}.{sig_b64}")
}

/// Verify a JWT and return the `sub` (user_id) if valid.
pub fn verify_jwt(token: &str, secret: &str, now_unix: u64) -> Result<String, ServiceError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(ServiceError::Unauthorized("invalid JWT format".into()));
    }

    // Verify signature
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let expected_sig = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    let actual_sig = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| ServiceError::Unauthorized("invalid JWT signature encoding".into()))?;

    if expected_sig.len() != actual_sig.len()
        || !expected_sig
            .iter()
            .zip(actual_sig.iter())
            .all(|(a, b)| a == b)
    {
        return Err(ServiceError::Unauthorized("invalid JWT signature".into()));
    }

    // Decode payload
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| ServiceError::Unauthorized("invalid JWT payload encoding".into()))?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)
        .map_err(|_| ServiceError::Unauthorized("invalid JWT payload".into()))?;

    // Check expiry
    let exp = payload["exp"]
        .as_u64()
        .ok_or_else(|| ServiceError::Unauthorized("missing exp claim".into()))?;
    if now_unix > exp {
        return Err(ServiceError::Unauthorized("JWT expired".into()));
    }

    // Extract sub
    let sub = payload["sub"]
        .as_str()
        .ok_or_else(|| ServiceError::Unauthorized("missing sub claim".into()))?
        .to_string();

    Ok(sub)
}

/// Generate a secure random token (for refresh tokens). Returns hex-encoded.
pub fn generate_token() -> Result<String, ServiceError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| ServiceError::Internal(format!("RNG failure: {e}")))?;
    Ok(hex::encode(bytes))
}

/// Hash a token with SHA-256 for storage. Returns hex-encoded.
pub fn hash_token(token: &str) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(token.as_bytes());
    hex::encode(hash)
}

// ── Internal ────────────────────────────────────────────────────────────────

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}
