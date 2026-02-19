//! User / auth query builders.

use sea_query::{Asterisk, Expr, Func, Query, SqliteQueryBuilder};

use super::tables::{RefreshTokens, Users};

pub type Built = (String, sea_query::Values);

// ── User lookups ───────────────────────────────────────────────────────────

/// Find user by id.
pub fn get_by_id(user_id: &str) -> Built {
    Query::select()
        .columns([Users::Id, Users::Nickname, Users::Email])
        .from(Users::Table)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Find user by email for login (returns id, nickname, password_hash, password_salt).
pub fn get_by_email_for_login(email: &str) -> Built {
    Query::select()
        .columns([
            Users::Id,
            Users::Nickname,
            Users::PasswordHash,
            Users::PasswordSalt,
        ])
        .from(Users::Table)
        .and_where(Expr::col(Users::Email).eq(email))
        .build(SqliteQueryBuilder)
}

/// Check email existence.
pub fn email_exists(email: &str) -> Built {
    Query::select()
        .expr(Expr::expr(Func::count(Expr::col(Asterisk))).gt(0))
        .from(Users::Table)
        .and_where(Expr::col(Users::Email).eq(email))
        .build(SqliteQueryBuilder)
}

/// Find user by nickname (returns id).
pub fn get_by_nickname(nickname: &str) -> Built {
    Query::select()
        .column(Users::Id)
        .from(Users::Table)
        .and_where(Expr::col(Users::Nickname).eq(nickname))
        .build(SqliteQueryBuilder)
}

// ── User inserts ───────────────────────────────────────────────────────────

/// Insert user with email/password.
pub fn insert_with_email(
    id: &str,
    nickname: &str,
    api_key_placeholder: &str,
    email: &str,
    password_hash: &str,
    password_salt: &str,
) -> Built {
    Query::insert()
        .into_table(Users::Table)
        .columns([
            Users::Id,
            Users::Nickname,
            Users::ApiKey,
            Users::Email,
            Users::PasswordHash,
            Users::PasswordSalt,
        ])
        .values_panic([
            id.into(),
            nickname.into(),
            api_key_placeholder.into(),
            email.into(),
            password_hash.into(),
            password_salt.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Insert user from OAuth (no password).
pub fn insert_oauth(
    id: &str,
    nickname: &str,
    api_key_placeholder: &str,
    email: Option<&str>,
) -> Built {
    Query::insert()
        .into_table(Users::Table)
        .columns([Users::Id, Users::Nickname, Users::ApiKey, Users::Email])
        .values_panic([
            id.into(),
            nickname.into(),
            api_key_placeholder.into(),
            email.map(|s| s.to_string()).into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Get nickname by user id.
pub fn get_nickname(user_id: &str) -> Built {
    Query::select()
        .column(Users::Nickname)
        .from(Users::Table)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

// ── User updates ───────────────────────────────────────────────────────────

/// Update password.
pub fn update_password(user_id: &str, password_hash: &str, password_salt: &str) -> Built {
    Query::update()
        .table(Users::Table)
        .value(Users::PasswordHash, password_hash)
        .value(Users::PasswordSalt, password_salt)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Regenerate API key.
pub fn update_api_key(user_id: &str, api_key: &str) -> Built {
    Query::update()
        .table(Users::Table)
        .value(Users::ApiKey, api_key)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

// ── User settings queries ──────────────────────────────────────────────────

/// Get password hash/salt for a user.
pub fn get_password_fields(user_id: &str) -> Built {
    Query::select()
        .columns([Users::PasswordHash, Users::PasswordSalt])
        .from(Users::Table)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Get user created_at (for settings).
pub fn get_settings_fields(user_id: &str) -> Built {
    Query::select()
        .column(Users::CreatedAt)
        .from(Users::Table)
        .and_where(Expr::col(Users::Id).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Get user email and avatar (for settings).
pub fn get_email_avatar(user_id: &str) -> Built {
    // Complex subquery — keep as raw SQL with sea-query values
    let sql = "SELECT \"email\", (SELECT \"avatar_url\" FROM \"oauth_identities\" WHERE \"user_id\" = ? LIMIT 1) FROM \"users\" WHERE \"id\" = ?".to_string();
    let values = sea_query::Values(vec![user_id.into(), user_id.into()]);
    (sql, values)
}

// ── Refresh tokens ─────────────────────────────────────────────────────────

/// Insert refresh token.
pub fn insert_refresh_token(id: &str, user_id: &str, token_hash: &str, expires_at: &str) -> Built {
    Query::insert()
        .into_table(RefreshTokens::Table)
        .columns([
            RefreshTokens::Id,
            RefreshTokens::UserId,
            RefreshTokens::TokenHash,
            RefreshTokens::ExpiresAt,
        ])
        .values_panic([
            id.into(),
            user_id.into(),
            token_hash.into(),
            expires_at.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Lookup refresh token with user join.
pub fn lookup_refresh_token(token_hash: &str) -> Built {
    Query::select()
        .column((RefreshTokens::Table, RefreshTokens::Id))
        .column((RefreshTokens::Table, RefreshTokens::UserId))
        .column((RefreshTokens::Table, RefreshTokens::ExpiresAt))
        .column((Users::Table, Users::Nickname))
        .from(RefreshTokens::Table)
        .inner_join(
            Users::Table,
            Expr::col((Users::Table, Users::Id))
                .equals((RefreshTokens::Table, RefreshTokens::UserId)),
        )
        .and_where(Expr::col((RefreshTokens::Table, RefreshTokens::TokenHash)).eq(token_hash))
        .build(SqliteQueryBuilder)
}

/// Delete refresh token by hash.
pub fn delete_refresh_token(token_hash: &str) -> Built {
    Query::delete()
        .from_table(RefreshTokens::Table)
        .and_where(Expr::col(RefreshTokens::TokenHash).eq(token_hash))
        .build(SqliteQueryBuilder)
}

/// Delete refresh token by id.
pub fn delete_refresh_token_by_id(id: &str) -> Built {
    Query::delete()
        .from_table(RefreshTokens::Table)
        .and_where(Expr::col(RefreshTokens::Id).eq(id))
        .build(SqliteQueryBuilder)
}
