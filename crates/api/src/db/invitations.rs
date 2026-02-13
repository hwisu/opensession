//! Invitation query builders.

use sea_query::{Alias, Asterisk, Expr, Func, Query, SqliteQueryBuilder};

use super::tables::TeamInvitations;

pub type Built = (String, sea_query::Values);

/// INSERT a new invitation.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    id: &str,
    team_id: &str,
    email: Option<&str>,
    oauth_provider: Option<&str>,
    oauth_provider_username: Option<&str>,
    invited_by: &str,
    role: &str,
    expires_at: &str,
) -> Built {
    Query::insert()
        .into_table(TeamInvitations::Table)
        .columns([
            TeamInvitations::Id,
            TeamInvitations::TeamId,
            TeamInvitations::Email,
            TeamInvitations::OauthProvider,
            TeamInvitations::OauthProviderUsername,
            TeamInvitations::InvitedBy,
            TeamInvitations::Role,
            TeamInvitations::ExpiresAt,
        ])
        .values_panic([
            id.into(),
            team_id.into(),
            email.map(|s| s.to_string()).into(),
            oauth_provider.map(|s| s.to_string()).into(),
            oauth_provider_username.map(|s| s.to_string()).into(),
            invited_by.into(),
            role.into(),
            expires_at.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// List pending, non-expired invitations for a user by email or OAuth identity.
/// Params: email, user_id.
pub fn list_my(email: &str, user_id: &str) -> Built {
    // This query uses complex subquery logic — keep as raw SQL
    let sql = concat!(
        "SELECT ",
        "i.\"id\", i.\"team_id\", t.\"name\" AS \"team_name\", i.\"email\", ",
        "i.\"oauth_provider\", i.\"oauth_provider_username\", ",
        "u.\"nickname\" AS \"invited_by_nickname\", i.\"role\", i.\"status\", ",
        "i.\"created_at\", i.\"expires_at\" ",
        "FROM \"team_invitations\" i ",
        "INNER JOIN \"teams\" t ON t.\"id\" = i.\"team_id\" ",
        "INNER JOIN \"users\" u ON u.\"id\" = i.\"invited_by\" ",
        "WHERE i.\"status\" = 'pending' ",
        "AND i.\"expires_at\" > datetime('now') ",
        "AND ((i.\"email\" IS NOT NULL AND i.\"email\" = ?) ",
        "OR (i.\"oauth_provider\" IS NOT NULL AND i.\"oauth_provider_username\" IS NOT NULL ",
        "AND EXISTS (SELECT 1 FROM \"oauth_identities\" oi ",
        "WHERE oi.\"user_id\" = ? ",
        "AND oi.\"provider\" = i.\"oauth_provider\" ",
        "AND lower(oi.\"provider_username\") = lower(i.\"oauth_provider_username\")))) ",
        "ORDER BY i.\"created_at\" DESC",
    )
    .to_string();
    let values = sea_query::Values(vec![email.into(), user_id.into()]);
    (sql, values)
}

/// Lookup an invitation by id.
pub fn lookup(id: &str) -> Built {
    Query::select()
        .columns([
            TeamInvitations::Id,
            TeamInvitations::TeamId,
            TeamInvitations::Email,
            TeamInvitations::OauthProvider,
            TeamInvitations::OauthProviderUsername,
            TeamInvitations::Role,
            TeamInvitations::Status,
            TeamInvitations::ExpiresAt,
        ])
        .from(TeamInvitations::Table)
        .and_where(Expr::col(TeamInvitations::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Update an invitation's status.
pub fn update_status(id: &str, status: &str) -> Built {
    Query::update()
        .table(TeamInvitations::Table)
        .value(TeamInvitations::Status, status)
        .and_where(Expr::col(TeamInvitations::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Check for duplicate pending invitation by email.
pub fn dup_check_email(team_id: &str, email: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(TeamInvitations::Table)
        .and_where(Expr::col(TeamInvitations::TeamId).eq(team_id))
        .and_where(Expr::col(TeamInvitations::Email).eq(email))
        .and_where(Expr::col(TeamInvitations::Status).eq("pending"))
        .build(SqliteQueryBuilder)
}

/// Check for duplicate pending invitation by OAuth provider.
pub fn dup_check_oauth(team_id: &str, provider: &str, username: &str) -> Built {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from(TeamInvitations::Table)
        .and_where(Expr::col(TeamInvitations::TeamId).eq(team_id))
        .and_where(Expr::col(TeamInvitations::OauthProvider).eq(provider))
        .and_where(Expr::col(TeamInvitations::OauthProviderUsername).eq(username))
        .and_where(Expr::col(TeamInvitations::Status).eq("pending"))
        .build(SqliteQueryBuilder)
}
