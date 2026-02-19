//! Session query builders.

use sea_query::{Alias, Asterisk, Expr, Func, JoinType, Order, Query, SqliteQueryBuilder};

use super::tables::{SessionLinks, Sessions, Users};
use crate::SessionListQuery;

pub type Built = (String, sea_query::Values);

/// Result of building a paginated session list query.
pub struct BuiltSessionListQuery {
    pub count_query: Built,
    pub select_query: Built,
    pub page: u32,
    pub per_page: u32,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Add the standard session columns (+ nickname from users join) to a SELECT.
/// Column order must match `session_from_row()` positional mappers.
fn session_columns(q: &mut sea_query::SelectStatement) -> &mut sea_query::SelectStatement {
    q.column((Sessions::Table, Sessions::Id))
        .column((Sessions::Table, Sessions::UserId))
        .column((Users::Table, Users::Nickname))
        .column((Sessions::Table, Sessions::Tool))
        .column((Sessions::Table, Sessions::AgentProvider))
        .column((Sessions::Table, Sessions::AgentModel))
        .column((Sessions::Table, Sessions::Title))
        .column((Sessions::Table, Sessions::Description))
        .column((Sessions::Table, Sessions::Tags))
        .column((Sessions::Table, Sessions::CreatedAt))
        .column((Sessions::Table, Sessions::UploadedAt))
        .column((Sessions::Table, Sessions::MessageCount))
        .column((Sessions::Table, Sessions::TaskCount))
        .column((Sessions::Table, Sessions::EventCount))
        .column((Sessions::Table, Sessions::DurationSeconds))
        .column((Sessions::Table, Sessions::TotalInputTokens))
        .column((Sessions::Table, Sessions::TotalOutputTokens))
        .column((Sessions::Table, Sessions::GitRemote))
        .column((Sessions::Table, Sessions::GitBranch))
        .column((Sessions::Table, Sessions::GitCommit))
        .column((Sessions::Table, Sessions::GitRepoName))
        .column((Sessions::Table, Sessions::PrNumber))
        .column((Sessions::Table, Sessions::PrUrl))
        .column((Sessions::Table, Sessions::WorkingDirectory))
        .column((Sessions::Table, Sessions::FilesModified))
        .column((Sessions::Table, Sessions::FilesRead))
        .column((Sessions::Table, Sessions::HasErrors))
        .column((Sessions::Table, Sessions::MaxActiveAgents))
        .column((Sessions::Table, Sessions::SessionScore))
        .column((Sessions::Table, Sessions::ScorePlugin))
}

/// Base SELECT for session listings (with users JOIN).
fn session_select() -> sea_query::SelectStatement {
    let mut q = Query::select().to_owned();
    session_columns(&mut q);
    q.from(Sessions::Table)
        .left_join(
            Users::Table,
            Expr::col((Sessions::Table, Sessions::UserId)).equals((Users::Table, Users::Id)),
        )
        .to_owned()
}

// ── Queries ────────────────────────────────────────────────────────────────

/// Parameters for inserting a session.
pub struct InsertParams<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub team_id: &'a str,
    pub tool: &'a str,
    pub agent_provider: &'a str,
    pub agent_model: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub tags: &'a str,
    pub created_at: &'a str,
    pub message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub body_storage_key: &'a str,
    pub body_url: Option<&'a str>,
    pub git_remote: Option<&'a str>,
    pub git_branch: Option<&'a str>,
    pub git_commit: Option<&'a str>,
    pub git_repo_name: Option<&'a str>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<&'a str>,
    pub working_directory: Option<&'a str>,
    pub files_modified: Option<&'a str>,
    pub files_read: Option<&'a str>,
    pub has_errors: bool,
    pub max_active_agents: i64,
    pub session_score: i64,
    pub score_plugin: &'a str,
}

/// INSERT a new session.
pub fn insert(p: &InsertParams<'_>) -> Built {
    Query::insert()
        .into_table(Sessions::Table)
        .columns([
            Sessions::Id,
            Sessions::UserId,
            Sessions::TeamId,
            Sessions::Tool,
            Sessions::AgentProvider,
            Sessions::AgentModel,
            Sessions::Title,
            Sessions::Description,
            Sessions::Tags,
            Sessions::CreatedAt,
            Sessions::MessageCount,
            Sessions::TaskCount,
            Sessions::EventCount,
            Sessions::DurationSeconds,
            Sessions::TotalInputTokens,
            Sessions::TotalOutputTokens,
            Sessions::BodyStorageKey,
            Sessions::BodyUrl,
            Sessions::GitRemote,
            Sessions::GitBranch,
            Sessions::GitCommit,
            Sessions::GitRepoName,
            Sessions::PrNumber,
            Sessions::PrUrl,
            Sessions::WorkingDirectory,
            Sessions::FilesModified,
            Sessions::FilesRead,
            Sessions::HasErrors,
            Sessions::MaxActiveAgents,
            Sessions::SessionScore,
            Sessions::ScorePlugin,
        ])
        .values_panic([
            p.id.into(),
            p.user_id.into(),
            p.team_id.into(),
            p.tool.into(),
            p.agent_provider.into(),
            p.agent_model.into(),
            p.title.into(),
            p.description.into(),
            p.tags.into(),
            p.created_at.into(),
            p.message_count.into(),
            p.task_count.into(),
            p.event_count.into(),
            p.duration_seconds.into(),
            p.total_input_tokens.into(),
            p.total_output_tokens.into(),
            p.body_storage_key.into(),
            p.body_url.map(|s| s.to_string()).into(),
            p.git_remote.map(|s| s.to_string()).into(),
            p.git_branch.map(|s| s.to_string()).into(),
            p.git_commit.map(|s| s.to_string()).into(),
            p.git_repo_name.map(|s| s.to_string()).into(),
            p.pr_number.into(),
            p.pr_url.map(|s| s.to_string()).into(),
            p.working_directory.map(|s| s.to_string()).into(),
            p.files_modified.map(|s| s.to_string()).into(),
            p.files_read.map(|s| s.to_string()).into(),
            p.has_errors.into(),
            p.max_active_agents.into(),
            p.session_score.into(),
            p.score_plugin.into(),
        ])
        .build(SqliteQueryBuilder)
}

/// SELECT a single session by id (with users JOIN).
pub fn get_by_id(id: &str) -> Built {
    session_select()
        .and_where(Expr::col((Sessions::Table, Sessions::Id)).eq(id))
        .build(SqliteQueryBuilder)
}

/// SELECT `body_storage_key, body_url` for a session.
pub fn get_storage_info(id: &str) -> Built {
    Query::select()
        .column(Sessions::BodyStorageKey)
        .column(Sessions::BodyUrl)
        .from(Sessions::Table)
        .and_where(Expr::col(Sessions::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// Build paginated session list queries with dynamic filters.
pub fn list(q: &SessionListQuery) -> BuiltSessionListQuery {
    let per_page = q.per_page.clamp(1, 100);
    let offset = (q.page.saturating_sub(1)) * per_page;

    // Build shared WHERE conditions
    let mut count_q = Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .from_as(Sessions::Table, Alias::new("s"))
        .to_owned();

    let mut select_q = Query::select().to_owned();
    // Use table alias "s" for sessions, "u" for users
    select_q
        .column((Alias::new("s"), Sessions::Id))
        .column((Alias::new("s"), Sessions::UserId))
        .column((Alias::new("u"), Users::Nickname))
        .column((Alias::new("s"), Sessions::Tool))
        .column((Alias::new("s"), Sessions::AgentProvider))
        .column((Alias::new("s"), Sessions::AgentModel))
        .column((Alias::new("s"), Sessions::Title))
        .column((Alias::new("s"), Sessions::Description))
        .column((Alias::new("s"), Sessions::Tags))
        .column((Alias::new("s"), Sessions::CreatedAt))
        .column((Alias::new("s"), Sessions::UploadedAt))
        .column((Alias::new("s"), Sessions::MessageCount))
        .column((Alias::new("s"), Sessions::TaskCount))
        .column((Alias::new("s"), Sessions::EventCount))
        .column((Alias::new("s"), Sessions::DurationSeconds))
        .column((Alias::new("s"), Sessions::TotalInputTokens))
        .column((Alias::new("s"), Sessions::TotalOutputTokens))
        .column((Alias::new("s"), Sessions::GitRemote))
        .column((Alias::new("s"), Sessions::GitBranch))
        .column((Alias::new("s"), Sessions::GitCommit))
        .column((Alias::new("s"), Sessions::GitRepoName))
        .column((Alias::new("s"), Sessions::PrNumber))
        .column((Alias::new("s"), Sessions::PrUrl))
        .column((Alias::new("s"), Sessions::WorkingDirectory))
        .column((Alias::new("s"), Sessions::FilesModified))
        .column((Alias::new("s"), Sessions::FilesRead))
        .column((Alias::new("s"), Sessions::HasErrors))
        .column((Alias::new("s"), Sessions::MaxActiveAgents))
        .column((Alias::new("s"), Sessions::SessionScore))
        .column((Alias::new("s"), Sessions::ScorePlugin))
        .from_as(Sessions::Table, Alias::new("s"))
        .join_as(
            JoinType::LeftJoin,
            Users::Table,
            Alias::new("u"),
            Expr::col((Alias::new("u"), Users::Id)).equals((Alias::new("s"), Sessions::UserId)),
        );

    // Base filter: non-empty sessions
    let base_cond = Expr::col((Alias::new("s"), Sessions::EventCount))
        .gt(0)
        .or(Expr::col((Alias::new("s"), Sessions::MessageCount)).gt(0));
    count_q.and_where(base_cond.clone());
    select_q.and_where(base_cond);

    if let Some(ref tool) = q.tool {
        let cond = Expr::col((Alias::new("s"), Sessions::Tool)).eq(tool.as_str());
        count_q.and_where(cond.clone());
        select_q.and_where(cond);
    }

    if let Some(ref search) = q.search {
        let like = format!("%{search}%");
        let cond = Expr::col((Alias::new("s"), Sessions::Title))
            .like(&like)
            .or(Expr::col((Alias::new("s"), Sessions::Description)).like(&like))
            .or(Expr::col((Alias::new("s"), Sessions::Tags)).like(&like));
        count_q.and_where(cond.clone());
        select_q.and_where(cond);
    }

    if let Some(ref time_range) = q.time_range {
        let interval = match time_range {
            crate::TimeRange::Hours24 => Some("-1 day"),
            crate::TimeRange::Days7 => Some("-7 days"),
            crate::TimeRange::Days30 => Some("-30 days"),
            crate::TimeRange::All => None,
        };
        if let Some(interval) = interval {
            let cond =
                Expr::col((Alias::new("s"), Sessions::CreatedAt)).gte(Expr::cust_with_values::<
                    _,
                    sea_query::Value,
                    _,
                >(
                    "datetime('now', ?)",
                    [interval.into()],
                ));
            count_q.and_where(cond.clone());
            select_q.and_where(cond);
        }
    }

    // Sort
    match q.sort.as_ref().unwrap_or(&crate::SortOrder::Recent) {
        crate::SortOrder::Popular => {
            select_q
                .order_by((Alias::new("s"), Sessions::MessageCount), Order::Desc)
                .order_by((Alias::new("s"), Sessions::CreatedAt), Order::Desc);
        }
        crate::SortOrder::Longest => {
            select_q
                .order_by((Alias::new("s"), Sessions::DurationSeconds), Order::Desc)
                .order_by((Alias::new("s"), Sessions::CreatedAt), Order::Desc);
        }
        crate::SortOrder::Recent => {
            select_q.order_by((Alias::new("s"), Sessions::CreatedAt), Order::Desc);
        }
    }

    select_q.limit(per_page as u64).offset(offset as u64);

    BuiltSessionListQuery {
        count_query: count_q.build(SqliteQueryBuilder),
        select_query: select_q.build(SqliteQueryBuilder),
        page: q.page,
        per_page,
    }
}

/// INSERT a session link.
pub fn insert_link(session_id: &str, linked_session_id: &str, link_type: crate::LinkType) -> Built {
    // INSERT OR IGNORE
    let sql = "INSERT OR IGNORE INTO \"session_links\" (\"session_id\", \"linked_session_id\", \"link_type\") VALUES (?, ?, ?)".to_string();
    let values = sea_query::Values(vec![
        session_id.into(),
        linked_session_id.into(),
        link_type.as_str().into(),
    ]);
    (sql, values)
}

/// SELECT all links for a session (both directions).
pub fn links_by_session(session_id: &str) -> Built {
    Query::select()
        .column(SessionLinks::SessionId)
        .column(SessionLinks::LinkedSessionId)
        .column(SessionLinks::LinkType)
        .column(SessionLinks::CreatedAt)
        .from(SessionLinks::Table)
        .cond_where(
            sea_query::Cond::any()
                .add(Expr::col(SessionLinks::SessionId).eq(session_id))
                .add(Expr::col(SessionLinks::LinkedSessionId).eq(session_id)),
        )
        .build(SqliteQueryBuilder)
}

/// DELETE a session by id.
pub fn delete(id: &str) -> Built {
    Query::delete()
        .from_table(Sessions::Table)
        .and_where(Expr::col(Sessions::Id).eq(id))
        .build(SqliteQueryBuilder)
}

/// DELETE all links for a session.
pub fn delete_links(session_id: &str) -> Built {
    Query::delete()
        .from_table(SessionLinks::Table)
        .and_where(
            Expr::col(SessionLinks::SessionId)
                .eq(session_id)
                .or(Expr::col(SessionLinks::LinkedSessionId).eq(session_id)),
        )
        .build(SqliteQueryBuilder)
}

/// INSERT into FTS index for a newly inserted session.
/// Server-specific: D1 does not support FTS.
pub fn insert_fts(session_id: &str) -> Built {
    let sql = concat!(
        "INSERT INTO \"sessions_fts\" (\"rowid\", \"title\", \"description\", \"tags\") ",
        "SELECT \"rowid\", \"title\", \"description\", \"tags\" FROM \"sessions\" WHERE \"id\" = ?",
    )
    .to_string();
    let values = sea_query::Values(vec![session_id.into()]);
    (sql, values)
}

/// DELETE from FTS index for a session being removed.
/// Server-specific: D1 does not support FTS.
pub fn delete_fts(session_id: &str) -> Built {
    let sql = concat!(
        "DELETE FROM \"sessions_fts\" WHERE \"rowid\" IN ",
        "(SELECT \"rowid\" FROM \"sessions\" WHERE \"id\" = ?)",
    )
    .to_string();
    let values = sea_query::Values(vec![session_id.into()]);
    (sql, values)
}
