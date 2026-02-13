//! Team + member query builders.

use sea_query::{Alias, Asterisk, Expr, Func, Order, Query, SqliteQueryBuilder};

use super::tables::{TeamMembers, Teams, Users};

pub type Built = (String, sea_query::Values);

// ── Team columns helper ───────────────────────────────────────────────────

/// Column list for team SELECT queries (matches legacy `TEAM_COLUMNS` order).
fn team_columns(q: &mut sea_query::SelectStatement) -> &mut sea_query::SelectStatement {
    q.column((Teams::Table, Teams::Id))
        .column((Teams::Table, Teams::Name))
        .column((Teams::Table, Teams::Description))
        .column((Teams::Table, Teams::IsPublic))
        .column((Teams::Table, Teams::CreatedBy))
        .column((Teams::Table, Teams::CreatedAt))
}

// ── Team queries ──────────────────────────────────────────────────────────

/// INSERT a new team.
pub fn insert(
    id: &str,
    name: &str,
    description: Option<&str>,
    is_public: bool,
    created_by: &str,
) -> Built {
    Query::insert()
        .into_table(Teams::Table)
        .columns([
            Teams::Id,
            Teams::Name,
            Teams::Description,
            Teams::IsPublic,
            Teams::CreatedBy,
        ])
        .values_panic([
            id.into(),
            name.into(),
            description.map(|s| s.to_string()).into(),
            is_public.into(),
            created_by.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// SELECT a single team by id.
pub fn get_by_id(id: &str) -> Built {
    let mut q = Query::select().to_owned();
    team_columns(&mut q);
    q.from(Teams::Table)
        .and_where(Expr::col((Teams::Table, Teams::Id)).eq(id))
        .build(SqliteQueryBuilder)
}

/// List teams for a user (via team_members join).
pub fn list_my(user_id: &str) -> Built {
    let mut q = Query::select().to_owned();
    team_columns(&mut q);
    q.from(Teams::Table)
        .inner_join(
            TeamMembers::Table,
            Expr::col((TeamMembers::Table, TeamMembers::TeamId)).equals((Teams::Table, Teams::Id)),
        )
        .and_where(Expr::col((TeamMembers::Table, TeamMembers::UserId)).eq(user_id))
        .order_by((Teams::Table, Teams::CreatedAt), Order::Desc)
        .build(SqliteQueryBuilder)
}

/// Check if a team exists.
pub fn exists(id: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(Teams::Table)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Get a team's name by id.
pub fn get_name(id: &str) -> Built {
    Query::select()
        .column(Teams::Name)
        .from(Teams::Table)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Get a team's created_at timestamp.
pub fn get_created_at(id: &str) -> Built {
    Query::select()
        .column(Teams::CreatedAt)
        .from(Teams::Table)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

// ── Team updates ──────────────────────────────────────────────────────────

/// Update a team's name.
pub fn update_name(id: &str, name: &str) -> Built {
    Query::update()
        .table(Teams::Table)
        .value(Teams::Name, name)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Update a team's description.
pub fn update_description(id: &str, description: &str) -> Built {
    Query::update()
        .table(Teams::Table)
        .value(Teams::Description, description)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Update a team's visibility.
pub fn update_visibility(id: &str, is_public: bool) -> Built {
    Query::update()
        .table(Teams::Table)
        .value(Teams::IsPublic, is_public)
        .and_where(Expr::col(Teams::Id).eq(id))
        .build(SqliteQueryBuilder)
}

// ── Member queries ────────────────────────────────────────────────────────

/// INSERT a team member.
pub fn member_insert(team_id: &str, user_id: &str, role: &str) -> Built {
    Query::insert()
        .into_table(TeamMembers::Table)
        .columns([TeamMembers::TeamId, TeamMembers::UserId, TeamMembers::Role])
        .values_panic([team_id.into(), user_id.into(), role.into()])
        .build(SqliteQueryBuilder)
}

/// DELETE a team member.
pub fn member_delete(team_id: &str, user_id: &str) -> Built {
    Query::delete()
        .from_table(TeamMembers::Table)
        .and_where(Expr::col(TeamMembers::TeamId).eq(team_id))
        .and_where(Expr::col(TeamMembers::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// List members of a team (joins with users table).
pub fn member_list(team_id: &str) -> Built {
    Query::select()
        .column((TeamMembers::Table, TeamMembers::UserId))
        .column((Users::Table, Users::Nickname))
        .column((TeamMembers::Table, TeamMembers::Role))
        .column((TeamMembers::Table, TeamMembers::JoinedAt))
        .from(TeamMembers::Table)
        .inner_join(
            Users::Table,
            Expr::col((Users::Table, Users::Id)).equals((TeamMembers::Table, TeamMembers::UserId)),
        )
        .and_where(Expr::col((TeamMembers::Table, TeamMembers::TeamId)).eq(team_id))
        .order_by((TeamMembers::Table, TeamMembers::JoinedAt), Order::Asc)
        .build(SqliteQueryBuilder)
}

/// Check if a user is a member of a team.
pub fn member_exists(team_id: &str, user_id: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(TeamMembers::Table)
        .and_where(Expr::col(TeamMembers::TeamId).eq(team_id))
        .and_where(Expr::col(TeamMembers::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Get a team member's role.
pub fn member_role(team_id: &str, user_id: &str) -> Built {
    Query::select()
        .column(TeamMembers::Role)
        .from(TeamMembers::Table)
        .and_where(Expr::col(TeamMembers::TeamId).eq(team_id))
        .and_where(Expr::col(TeamMembers::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}

/// Count members in a team.
pub fn member_count(team_id: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(TeamMembers::Table)
        .and_where(Expr::col(TeamMembers::TeamId).eq(team_id))
        .build(SqliteQueryBuilder)
}

/// Get a team member's joined_at timestamp.
pub fn member_joined_at(team_id: &str, user_id: &str) -> Built {
    Query::select()
        .column(TeamMembers::JoinedAt)
        .from(TeamMembers::Table)
        .and_where(Expr::col(TeamMembers::TeamId).eq(team_id))
        .and_where(Expr::col(TeamMembers::UserId).eq(user_id))
        .build(SqliteQueryBuilder)
}
