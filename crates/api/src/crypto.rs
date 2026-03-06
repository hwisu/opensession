//! Cryptographic helpers for authentication.
//!
//! - PBKDF2-SHA256 password hashing (600k iterations)
//! - HMAC-SHA256 JWT signing/verification
//!
//! Uses pure Rust crates (wasm-compatible, no WebCrypto interop needed).

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::collections::HashMap;

use crate::ServiceError;

const PBKDF2_ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const ENVELOPE_VERSION: &str = "v1";

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

    constant_time_eq(hash.as_slice(), expected.as_slice())
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

    if !constant_time_eq(expected_sig.as_slice(), actual_sig.as_slice()) {
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

// ── Envelope encryption for credential storage ────────────────────────────

/// Envelope keyring used for encrypting/decrypting stored credential secrets.
///
/// Key format:
/// - Key set input: `kid1:<64hex>,kid2:<64hex>`
/// - Active key id picks which master key encrypts new records.
#[derive(Clone, Debug)]
pub struct CredentialKeyring {
    pub active_kid: String,
    keys: HashMap<String, [u8; 32]>,
}

impl CredentialKeyring {
    /// Build a keyring from env-like values.
    pub fn from_csv(active_kid: &str, keys_csv: &str) -> Result<Self, ServiceError> {
        let mut keys = HashMap::<String, [u8; 32]>::new();
        for entry in keys_csv
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let (kid, key_hex) = entry.split_once(':').ok_or_else(|| {
                ServiceError::BadRequest("credential key entry must be `<kid>:<hex>`".into())
            })?;
            let kid = kid.trim();
            if kid.is_empty() {
                return Err(ServiceError::BadRequest(
                    "credential key id cannot be empty".into(),
                ));
            }
            let raw = hex::decode(key_hex.trim()).map_err(|_| {
                ServiceError::BadRequest(format!("credential key `{kid}` is not valid hex"))
            })?;
            if raw.len() != 32 {
                return Err(ServiceError::BadRequest(format!(
                    "credential key `{kid}` must be 32 bytes (64 hex chars)"
                )));
            }
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&raw);
            keys.insert(kid.to_string(), buf);
        }

        if keys.is_empty() {
            return Err(ServiceError::BadRequest(
                "credential key set is empty".into(),
            ));
        }
        let active_kid = active_kid.trim().to_string();
        if active_kid.is_empty() {
            return Err(ServiceError::BadRequest(
                "active credential key id is empty".into(),
            ));
        }
        if !keys.contains_key(&active_kid) {
            return Err(ServiceError::BadRequest(format!(
                "active credential key id `{active_kid}` is missing in key set"
            )));
        }

        Ok(Self { active_kid, keys })
    }

    /// Encrypt plaintext using envelope encryption with per-record random DEK.
    ///
    /// Output format:
    /// `v1:<kid>:<wrap_nonce_b64>:<wrapped_dek_b64>:<data_nonce_b64>:<ciphertext_b64>`
    pub fn encrypt(&self, plaintext: &str) -> Result<String, ServiceError> {
        let master = self
            .keys
            .get(&self.active_kid)
            .ok_or_else(|| ServiceError::Internal("active credential key is not loaded".into()))?;

        let mut dek = [0u8; 32];
        getrandom::getrandom(&mut dek)
            .map_err(|e| ServiceError::Internal(format!("RNG failure: {e}")))?;

        let mut wrap_nonce = [0u8; 24];
        getrandom::getrandom(&mut wrap_nonce)
            .map_err(|e| ServiceError::Internal(format!("RNG failure: {e}")))?;
        let mut data_nonce = [0u8; 24];
        getrandom::getrandom(&mut data_nonce)
            .map_err(|e| ServiceError::Internal(format!("RNG failure: {e}")))?;

        let wrap_cipher = XChaCha20Poly1305::new_from_slice(master)
            .map_err(|_| ServiceError::Internal("invalid master key length".into()))?;
        let wrapped_dek = wrap_cipher
            .encrypt(XNonce::from_slice(&wrap_nonce), dek.as_slice())
            .map_err(|_| ServiceError::Internal("failed to encrypt credential DEK".into()))?;

        let data_cipher = XChaCha20Poly1305::new_from_slice(&dek)
            .map_err(|_| ServiceError::Internal("invalid DEK length".into()))?;
        let ciphertext = data_cipher
            .encrypt(XNonce::from_slice(&data_nonce), plaintext.as_bytes())
            .map_err(|_| ServiceError::Internal("failed to encrypt credential payload".into()))?;

        Ok(format!(
            "{ENVELOPE_VERSION}:{}:{}:{}:{}:{}",
            self.active_kid,
            URL_SAFE_NO_PAD.encode(wrap_nonce),
            URL_SAFE_NO_PAD.encode(wrapped_dek),
            URL_SAFE_NO_PAD.encode(data_nonce),
            URL_SAFE_NO_PAD.encode(ciphertext),
        ))
    }

    /// Decrypt a previously encrypted credential payload.
    pub fn decrypt(&self, encoded: &str) -> Result<String, ServiceError> {
        let mut parts = encoded.split(':');
        let version = parts.next().unwrap_or_default();
        let kid = parts.next().unwrap_or_default();
        let wrap_nonce_b64 = parts.next().unwrap_or_default();
        let wrapped_dek_b64 = parts.next().unwrap_or_default();
        let data_nonce_b64 = parts.next().unwrap_or_default();
        let ciphertext_b64 = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || version != ENVELOPE_VERSION
            || kid.is_empty()
            || wrap_nonce_b64.is_empty()
            || wrapped_dek_b64.is_empty()
            || data_nonce_b64.is_empty()
            || ciphertext_b64.is_empty()
        {
            return Err(ServiceError::BadRequest(
                "invalid encrypted credential format".into(),
            ));
        }

        let master = self.keys.get(kid).ok_or_else(|| {
            ServiceError::BadRequest(format!("unknown credential key id `{kid}`"))
        })?;

        let wrap_nonce = URL_SAFE_NO_PAD
            .decode(wrap_nonce_b64)
            .map_err(|_| ServiceError::BadRequest("invalid wrap nonce encoding".into()))?;
        let wrapped_dek = URL_SAFE_NO_PAD
            .decode(wrapped_dek_b64)
            .map_err(|_| ServiceError::BadRequest("invalid wrapped DEK encoding".into()))?;
        let data_nonce = URL_SAFE_NO_PAD
            .decode(data_nonce_b64)
            .map_err(|_| ServiceError::BadRequest("invalid data nonce encoding".into()))?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext_b64)
            .map_err(|_| ServiceError::BadRequest("invalid ciphertext encoding".into()))?;

        if wrap_nonce.len() != 24 || data_nonce.len() != 24 {
            return Err(ServiceError::BadRequest(
                "encrypted credential nonce length is invalid".into(),
            ));
        }

        let wrap_cipher = XChaCha20Poly1305::new_from_slice(master)
            .map_err(|_| ServiceError::Internal("invalid master key length".into()))?;
        let dek = wrap_cipher
            .decrypt(XNonce::from_slice(&wrap_nonce), wrapped_dek.as_slice())
            .map_err(|_| ServiceError::BadRequest("failed to decrypt credential DEK".into()))?;
        if dek.len() != 32 {
            return Err(ServiceError::BadRequest(
                "credential DEK length is invalid".into(),
            ));
        }

        let data_cipher = XChaCha20Poly1305::new_from_slice(&dek)
            .map_err(|_| ServiceError::Internal("invalid DEK length".into()))?;
        let plain = data_cipher
            .decrypt(XNonce::from_slice(&data_nonce), ciphertext.as_slice())
            .map_err(|_| ServiceError::BadRequest("failed to decrypt credential payload".into()))?;
        String::from_utf8(plain)
            .map_err(|_| ServiceError::BadRequest("credential payload is not UTF-8".into()))
    }
}

// ── Internal ────────────────────────────────────────────────────────────────

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn constant_time_eq(lhs: &[u8], rhs: &[u8]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&a, &b) in lhs.iter().zip(rhs.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{CredentialKeyring, constant_time_eq};

    #[test]
    fn credential_keyring_round_trip_encrypt_decrypt() {
        let keyring = CredentialKeyring::from_csv(
            "k1",
            "k1:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        )
        .expect("keyring");
        let encrypted = keyring.encrypt("secret-token-value").expect("encrypt");
        let decrypted = keyring.decrypt(&encrypted).expect("decrypt");
        assert_eq!(decrypted, "secret-token-value");
    }

    #[test]
    fn credential_keyring_rejects_missing_active_key() {
        let err = CredentialKeyring::from_csv(
            "missing",
            "k1:00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        )
        .expect_err("missing active key should fail");
        assert!(
            err.message()
                .contains("active credential key id `missing` is missing")
        );
    }

    #[test]
    fn constant_time_eq_matches_expected_behavior() {
        assert!(constant_time_eq(b"abc123", b"abc123"));
        assert!(!constant_time_eq(b"abc123", b"abc124"));
        assert!(!constant_time_eq(b"abc123", b"abc1234"));
    }
}
