//! User-managed git credential query builders.

use sea_query::{Expr, Order, Query, SqliteQueryBuilder};

use super::tables::GitCredentials;

pub type Built = (String, sea_query::Values);

/// List credential metadata for a user.
pub fn list_by_user(user_id: &str) -> Built {
    Query::select()
        .columns([
            GitCredentials::Id,
            GitCredentials::Label,
            GitCredentials::Host,
            GitCredentials::PathPrefix,
            GitCredentials::HeaderName,
            GitCredentials::CreatedAt,
            GitCredentials::UpdatedAt,
            GitCredentials::LastUsedAt,
        ])
        .from(GitCredentials::Table)
        .and_where(Expr::col(GitCredentials::UserId).eq(user_id))
        .order_by(GitCredentials::CreatedAt, Order::Desc)
        .build(SqliteQueryBuilder)
}

/// Get a single credential summary row by id + owner.
pub fn get_by_id_and_user(id: &str, user_id: &str) -> Built {
    Query::select()
        .columns([
            GitCredentials::Id,
            GitCredentials::Label,
            GitCredentials::Host,
            GitCredentials::PathPrefix,
            GitCredentials::HeaderName,
            GitCredentials::CreatedAt,
            GitCredentials::UpdatedAt,
            GitCredentials::LastUsedAt,
        ])
        .from(GitCredentials::Table)
        .and_where(Expr::col(GitCredentials::Id).eq(id))
        .and_where(Expr::col(GitCredentials::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// List credentials for host matching and include encrypted secret value.
///
/// Sorted by longest path prefix first for deterministic best-match selection.
pub fn list_for_host_with_secret(user_id: &str, host: &str) -> Built {
    let sql = concat!(
        "SELECT \"id\", \"label\", \"host\", \"path_prefix\", \"header_name\", \"header_value_enc\", ",
        "\"created_at\", \"updated_at\", \"last_used_at\" ",
        "FROM \"git_credentials\" ",
        "WHERE \"user_id\" = ? AND lower(\"host\") = lower(?) ",
        "ORDER BY length(\"path_prefix\") DESC, \"created_at\" DESC"
    )
    .to_string();
    let values = sea_query::Values(vec![user_id.into(), host.to_string().into()]);
    (sql, values)
}

/// Insert a new credential.
pub fn insert(
    id: &str,
    user_id: &str,
    label: &str,
    host: &str,
    path_prefix: &str,
    header_name: &str,
    header_value_enc: &str,
) -> Built {
    Query::insert()
        .into_table(GitCredentials::Table)
        .columns([
            GitCredentials::Id,
            GitCredentials::UserId,
            GitCredentials::Label,
            GitCredentials::Host,
            GitCredentials::PathPrefix,
            GitCredentials::HeaderName,
            GitCredentials::HeaderValueEnc,
        ])
        .values_panic([
            id.into(),
            user_id.into(),
            label.into(),
            host.into(),
            path_prefix.into(),
            header_name.into(),
            header_value_enc.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Delete a credential owned by the user.
pub fn delete_by_id_and_user(id: &str, user_id: &str) -> Built {
    Query::delete()
        .from_table(GitCredentials::Table)
        .and_where(Expr::col(GitCredentials::Id).eq(id))
        .and_where(Expr::col(GitCredentials::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Update `last_used_at` when a credential is consumed for a fetch.
pub fn touch_last_used(id: &str, user_id: &str) -> Built {
    Query::update()
        .table(GitCredentials::Table)
        .value(GitCredentials::LastUsedAt, Expr::cust("datetime('now')"))
        .value(GitCredentials::UpdatedAt, Expr::cust("datetime('now')"))
        .and_where(Expr::col(GitCredentials::Id).eq(id))
        .and_where(Expr::col(GitCredentials::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}
