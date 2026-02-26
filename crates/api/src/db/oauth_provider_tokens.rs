//! OAuth provider token query builders.

use sea_query::{Expr, Query, SqliteQueryBuilder};

use super::tables::OauthProviderTokens;

pub type Built = (String, sea_query::Values);

/// Upsert provider access token for a user.
pub fn upsert_access_token(
    id: &str,
    user_id: &str,
    provider: &str,
    provider_host: &str,
    access_token_enc: &str,
    expires_at: Option<&str>,
) -> Built {
    let sql = concat!(
        "INSERT INTO \"oauth_provider_tokens\" ",
        "(\"id\", \"user_id\", \"provider\", \"provider_host\", \"access_token_enc\", \"expires_at\") ",
        "VALUES (?, ?, ?, ?, ?, ?) ",
        "ON CONFLICT (\"user_id\", \"provider\", \"provider_host\") DO UPDATE SET ",
        "\"access_token_enc\" = excluded.\"access_token_enc\", ",
        "\"expires_at\" = excluded.\"expires_at\", ",
        "\"updated_at\" = datetime('now')"
    )
    .to_string();
    let values = sea_query::Values(vec![
        id.into(),
        user_id.into(),
        provider.into(),
        provider_host.into(),
        access_token_enc.into(),
        expires_at.map(|v| v.to_string()).into(),
    ]);
    (sql, values)
}

/// Read provider token for a user + provider host.
pub fn get_by_user_provider_host(user_id: &str, provider: &str, provider_host: &str) -> Built {
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
        .and_where(Expr::col(OauthProviderTokens::ProviderHost).eq(provider_host))
        .build(SqliteQueryBuilder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_uses_host_bound_conflict_key() {
        let (sql, values) = upsert_access_token(
            "token-id",
            "user-id",
            "gitlab",
            "gitlab.example.com",
            "enc-value",
            None,
        );
        assert!(sql.contains("\"provider_host\""));
        assert!(sql.contains("ON CONFLICT (\"user_id\", \"provider\", \"provider_host\")"));
        assert_eq!(values.0.len(), 6);
    }

    #[test]
    fn get_query_filters_by_host() {
        let (sql, _values) = get_by_user_provider_host("user-id", "gitlab", "gitlab.example.com");
        assert!(sql.contains("\"provider_host\""));
        assert!(sql.contains("WHERE"));
    }
}
