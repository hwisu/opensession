//! OAuth provider token query builders.

use sea_query::{Expr, Query, SqliteQueryBuilder};

use super::tables::OauthProviderTokens;

pub type Built = (String, sea_query::Values);

/// Upsert provider access token for a user.
pub fn upsert_access_token(
    id: &str,
    user_id: &str,
    provider: &str,
    access_token_enc: &str,
    expires_at: Option<&str>,
) -> Built {
    let sql = concat!(
        "INSERT INTO \"oauth_provider_tokens\" ",
        "(\"id\", \"user_id\", \"provider\", \"access_token_enc\", \"expires_at\") ",
        "VALUES (?, ?, ?, ?, ?) ",
        "ON CONFLICT (\"user_id\", \"provider\") DO UPDATE SET ",
        "\"access_token_enc\" = excluded.\"access_token_enc\", ",
        "\"expires_at\" = excluded.\"expires_at\", ",
        "\"updated_at\" = datetime('now')"
    )
    .to_string();
    let values = sea_query::Values(vec![
        id.into(),
        user_id.into(),
        provider.into(),
        access_token_enc.into(),
        expires_at.map(|v| v.to_string()).into(),
    ]);
    (sql, values)
}

/// Read provider token for a user.
pub fn get_by_user_provider(user_id: &str, provider: &str) -> Built {
    Query::select()
        .columns([
            OauthProviderTokens::Id,
            OauthProviderTokens::AccessTokenEnc,
            OauthProviderTokens::ExpiresAt,
            OauthProviderTokens::UpdatedAt,
        ])
        .from(OauthProviderTokens::Table)
        .and_where(Expr::col(OauthProviderTokens::UserId).eq(user_id))
        .and_where(Expr::col(OauthProviderTokens::Provider).eq(provider))
        .build(SqliteQueryBuilder)
}
