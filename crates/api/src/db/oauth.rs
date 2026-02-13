//! OAuth identity / state query builders.

use sea_query::{Alias, Asterisk, Expr, Func, Query, SqliteQueryBuilder};

use super::tables::{OauthIdentities, OauthStates};

pub type Built = (String, sea_query::Values);

// ── OAuth Identities ──────────────────────────────────────────────────────

/// UPSERT an OAuth identity (link or update).
pub fn upsert_identity(
    user_id: &str,
    provider: &str,
    provider_user_id: &str,
    provider_username: Option<&str>,
    avatar_url: Option<&str>,
    instance_url: Option<&str>,
) -> Built {
    // ON CONFLICT requires raw SQL — sea-query's ON CONFLICT support is limited
    let sql = concat!(
        "INSERT INTO \"oauth_identities\" ",
        "(\"user_id\", \"provider\", \"provider_user_id\", \"provider_username\", \"avatar_url\", \"instance_url\") ",
        "VALUES (?, ?, ?, ?, ?, ?) ",
        "ON CONFLICT (\"provider\", \"provider_user_id\") DO UPDATE SET ",
        "\"provider_username\" = excluded.\"provider_username\", ",
        "\"avatar_url\" = excluded.\"avatar_url\"",
    )
    .to_string();
    let values = sea_query::Values(vec![
        user_id.into(),
        provider.into(),
        provider_user_id.into(),
        provider_username.map(|s| s.to_string()).into(),
        avatar_url.map(|s| s.to_string()).into(),
        instance_url.map(|s| s.to_string()).into(),
    ]);
    (sql, values)
}

/// Find a user by OAuth identity.
pub fn find_by_provider(provider: &str, provider_user_id: &str) -> Built {
    Query::select()
        .columns([
            OauthIdentities::UserId,
            OauthIdentities::Provider,
            OauthIdentities::ProviderUserId,
            OauthIdentities::ProviderUsername,
            OauthIdentities::AvatarUrl,
            OauthIdentities::InstanceUrl,
        ])
        .from(OauthIdentities::Table)
        .and_where(Expr::col(OauthIdentities::Provider).eq(provider))
        .and_where(Expr::col(OauthIdentities::ProviderUserId).eq(provider_user_id))
        .build(SqliteQueryBuilder)
}

/// Find all OAuth identities for a user.
pub fn find_by_user(user_id: &str) -> Built {
    Query::select()
        .columns([
            OauthIdentities::UserId,
            OauthIdentities::Provider,
            OauthIdentities::ProviderUserId,
            OauthIdentities::ProviderUsername,
            OauthIdentities::AvatarUrl,
            OauthIdentities::InstanceUrl,
        ])
        .from(OauthIdentities::Table)
        .and_where(Expr::col(OauthIdentities::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Check if a user has a matching OAuth identity.
pub fn identity_match(user_id: &str, provider: &str, provider_username: &str) -> Built {
    // Case-insensitive match on provider_username
    let sql = concat!(
        "SELECT COUNT(*) AS \"count\" FROM \"oauth_identities\" ",
        "WHERE \"user_id\" = ? AND \"provider\" = ? AND lower(\"provider_username\") = lower(?)",
    )
    .to_string();
    let values = sea_query::Values(vec![
        user_id.into(),
        provider.into(),
        provider_username.into(),
    ]);
    (sql, values)
}

// ── OAuth States ──────────────────────────────────────────────────────────

/// Insert an OAuth state token.
pub fn insert_state(state: &str, provider: &str, expires_at: &str, user_id: Option<&str>) -> Built {
    Query::insert()
        .into_table(OauthStates::Table)
        .columns([
            OauthStates::State,
            OauthStates::Provider,
            OauthStates::ExpiresAt,
            OauthStates::UserId,
        ])
        .values_panic([
            state.into(),
            provider.into(),
            expires_at.into(),
            user_id.map(|s| s.to_string()).into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Validate and retrieve an OAuth state token.
pub fn validate_state(state: &str) -> Built {
    Query::select()
        .columns([
            OauthStates::State,
            OauthStates::Provider,
            OauthStates::ExpiresAt,
            OauthStates::UserId,
        ])
        .from(OauthStates::Table)
        .and_where(Expr::col(OauthStates::State).eq(state))
        .build(SqliteQueryBuilder)
}

/// Check if a user has a specific provider linked (returns count).
pub fn has_provider(user_id: &str, provider: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(OauthIdentities::Table)
        .and_where(Expr::col(OauthIdentities::UserId).eq(user_id))
        .and_where(Expr::col(OauthIdentities::Provider).eq(provider))
        .build(SqliteQueryBuilder)
}

/// Delete a used OAuth state token.
pub fn delete_state(state: &str) -> Built {
    Query::delete()
        .from_table(OauthStates::Table)
        .and_where(Expr::col(OauthStates::State).eq(state))
        .build(SqliteQueryBuilder)
}
