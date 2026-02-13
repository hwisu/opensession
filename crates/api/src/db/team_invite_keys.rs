//! Team invite key query builders.

use sea_query::{Expr, Order, Query, SqliteQueryBuilder};

use super::tables::{TeamInviteKeys, Teams, Users};

pub type Built = (String, sea_query::Values);

pub fn insert(
    id: &str,
    team_id: &str,
    key_hash: &str,
    role: &str,
    created_by: &str,
    expires_at: &str,
) -> Built {
    Query::insert()
        .into_table(TeamInviteKeys::Table)
        .columns([
            TeamInviteKeys::Id,
            TeamInviteKeys::TeamId,
            TeamInviteKeys::KeyHash,
            TeamInviteKeys::Role,
            TeamInviteKeys::CreatedBy,
            TeamInviteKeys::ExpiresAt,
        ])
        .values_panic([
            id.into(),
            team_id.into(),
            key_hash.into(),
            role.into(),
            created_by.into(),
            expires_at.into(),
        ])
        .build(SqliteQueryBuilder)
}

pub fn list_for_team(team_id: &str) -> Built {
    Query::select()
        .column((TeamInviteKeys::Table, TeamInviteKeys::Id))
        .column((TeamInviteKeys::Table, TeamInviteKeys::Role))
        .column((Users::Table, Users::Nickname))
        .column((TeamInviteKeys::Table, TeamInviteKeys::CreatedAt))
        .column((TeamInviteKeys::Table, TeamInviteKeys::ExpiresAt))
        .column((TeamInviteKeys::Table, TeamInviteKeys::UsedAt))
        .column((TeamInviteKeys::Table, TeamInviteKeys::RevokedAt))
        .from(TeamInviteKeys::Table)
        .inner_join(
            Users::Table,
            Expr::col((Users::Table, Users::Id))
                .equals((TeamInviteKeys::Table, TeamInviteKeys::CreatedBy)),
        )
        .and_where(Expr::col((TeamInviteKeys::Table, TeamInviteKeys::TeamId)).eq(team_id))
        .order_by(
            (TeamInviteKeys::Table, TeamInviteKeys::CreatedAt),
            Order::Desc,
        )
        .build(SqliteQueryBuilder)
}

pub fn lookup_active_by_hash(hash: &str) -> Built {
    Query::select()
        .column(TeamInviteKeys::Id)
        .column(TeamInviteKeys::TeamId)
        .column(TeamInviteKeys::Role)
        .column(TeamInviteKeys::ExpiresAt)
        .column(TeamInviteKeys::UsedAt)
        .column(TeamInviteKeys::RevokedAt)
        .from(TeamInviteKeys::Table)
        .and_where(Expr::col(TeamInviteKeys::KeyHash).eq(hash))
        .limit(1)
        .build(SqliteQueryBuilder)
}

pub fn mark_used(id: &str, user_id: &str) -> Built {
    Query::update()
        .table(TeamInviteKeys::Table)
        .value(TeamInviteKeys::UsedBy, user_id)
        .value(TeamInviteKeys::UsedAt, Expr::cust("datetime('now')"))
        .and_where(Expr::col(TeamInviteKeys::Id).eq(id))
        .build(SqliteQueryBuilder)
}

pub fn revoke(team_id: &str, key_id: &str) -> Built {
    Query::update()
        .table(TeamInviteKeys::Table)
        .value(TeamInviteKeys::RevokedAt, Expr::cust("datetime('now')"))
        .and_where(Expr::col(TeamInviteKeys::TeamId).eq(team_id))
        .and_where(Expr::col(TeamInviteKeys::Id).eq(key_id))
        .and_where(Expr::col(TeamInviteKeys::UsedAt).is_null())
        .and_where(Expr::col(TeamInviteKeys::RevokedAt).is_null())
        .build(SqliteQueryBuilder)
}

pub fn team_name(team_id: &str) -> Built {
    Query::select()
        .column((Teams::Table, Teams::Name))
        .from(Teams::Table)
        .and_where(Expr::col((Teams::Table, Teams::Id)).eq(team_id))
        .limit(1)
        .build(SqliteQueryBuilder)
}
