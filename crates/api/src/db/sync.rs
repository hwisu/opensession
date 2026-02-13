//! Sync-tracking query builders (TUI + Daemon).

use sea_query::{Expr, Query, SqliteQueryBuilder};

use super::tables::{BodyCache, SessionSync, SyncCursors};

pub type Built = (String, sea_query::Values);

// ── Session sync ─────────────────────────────────────────────────────────

/// Get sync status for a session.
pub fn get_sync_status(session_id: &str) -> Built {
    Query::select()
        .column(SessionSync::SyncStatus)
        .column(SessionSync::SourcePath)
        .column(SessionSync::LastSyncedAt)
        .from(SessionSync::Table)
        .and_where(Expr::col(SessionSync::SessionId).eq(session_id))
        .build(SqliteQueryBuilder)
}

/// Pending uploads: sessions with sync_status='local_only'.
pub fn pending_uploads() -> Built {
    Query::select()
        .column(SessionSync::SessionId)
        .column(SessionSync::SourcePath)
        .from(SessionSync::Table)
        .and_where(Expr::col(SessionSync::SyncStatus).eq("local_only"))
        .build(SqliteQueryBuilder)
}

// ── Sync cursors ──────────────────────────────────────────────────────────

/// Get sync cursor for a team.
pub fn get_sync_cursor(team_id: &str) -> Built {
    Query::select()
        .column(SyncCursors::Cursor)
        .from(SyncCursors::Table)
        .and_where(Expr::col(SyncCursors::TeamId).eq(team_id))
        .build(SqliteQueryBuilder)
}

/// Set sync cursor for a team (INSERT OR REPLACE).
pub fn set_sync_cursor(team_id: &str, cursor: &str) -> Built {
    let sql = "INSERT OR REPLACE INTO \"sync_cursors\" (\"team_id\", \"cursor\") VALUES (?, ?)"
        .to_string();
    let values = sea_query::Values(vec![team_id.into(), cursor.into()]);
    (sql, values)
}

// ── Body cache ────────────────────────────────────────────────────────────

/// Cache a session body.
pub fn cache_body(session_id: &str, body: &[u8]) -> Built {
    let sql = "INSERT OR REPLACE INTO \"body_cache\" (\"session_id\", \"body\") VALUES (?, ?)"
        .to_string();
    let values = sea_query::Values(vec![
        session_id.into(),
        sea_query::Value::Bytes(Some(Box::new(body.to_vec()))),
    ]);
    (sql, values)
}

/// Get cached session body.
pub fn get_cached_body(session_id: &str) -> Built {
    Query::select()
        .column(BodyCache::Body)
        .from(BodyCache::Table)
        .and_where(Expr::col(BodyCache::SessionId).eq(session_id))
        .build(SqliteQueryBuilder)
}
