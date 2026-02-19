//! API key query builders.

use sea_query::{Expr, OnConflict, Query, SqliteQueryBuilder};

use super::tables::{ApiKeys, Users};

pub type Built = (String, sea_query::Values);

/// Lookup user by API key hash and validity window.
pub fn get_user_by_valid_key_hash(key_hash: &str) -> Built {
    Query::select()
        .column((Users::Table, Users::Id))
        .column((Users::Table, Users::Nickname))
        .column((Users::Table, Users::Email))
        .from(ApiKeys::Table)
        .inner_join(
            Users::Table,
            Expr::col((Users::Table, Users::Id)).equals((ApiKeys::Table, ApiKeys::UserId)),
        )
        .and_where(Expr::col((ApiKeys::Table, ApiKeys::KeyHash)).eq(key_hash))
        .and_where(
            Expr::col((ApiKeys::Table, ApiKeys::Status))
                .eq("active")
                .or(Expr::col((ApiKeys::Table, ApiKeys::Status))
                    .eq("grace")
                    .and(
                        Expr::col((ApiKeys::Table, ApiKeys::GraceUntil))
                            .gt(Expr::cust("datetime('now')")),
                    )),
        )
        .build(SqliteQueryBuilder)
}

/// Insert an active API key row.
pub fn insert_active(id: &str, user_id: &str, key_hash: &str, key_prefix: &str) -> Built {
    Query::insert()
        .into_table(ApiKeys::Table)
        .columns([
            ApiKeys::Id,
            ApiKeys::UserId,
            ApiKeys::KeyHash,
            ApiKeys::KeyPrefix,
            ApiKeys::Status,
        ])
        .values_panic([
            id.into(),
            user_id.into(),
            key_hash.into(),
            key_prefix.into(),
            "active".into(),
        ])
        .build(SqliteQueryBuilder)
}

/// Insert an active API key row if not present.
pub fn insert_active_if_missing(
    id: &str,
    user_id: &str,
    key_hash: &str,
    key_prefix: &str,
) -> Built {
    Query::insert()
        .into_table(ApiKeys::Table)
        .columns([
            ApiKeys::Id,
            ApiKeys::UserId,
            ApiKeys::KeyHash,
            ApiKeys::KeyPrefix,
            ApiKeys::Status,
        ])
        .values_panic([
            id.into(),
            user_id.into(),
            key_hash.into(),
            key_prefix.into(),
            "active".into(),
        ])
        .on_conflict(OnConflict::column(ApiKeys::KeyHash).do_nothing().to_owned())
        .build(SqliteQueryBuilder)
}

/// Move active keys to grace state for a user.
pub fn move_active_to_grace(user_id: &str, grace_until: &str) -> Built {
    Query::update()
        .table(ApiKeys::Table)
        .value(ApiKeys::Status, "grace")
        .value(ApiKeys::GraceUntil, grace_until)
        .and_where(Expr::col(ApiKeys::UserId).eq(user_id))
        .and_where(Expr::col(ApiKeys::Status).eq("active"))
        .build(SqliteQueryBuilder)
}
