use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use opensession_api_types::db;
use opensession_api_types::{SessionSummary, TeamResponse};

/// Shared database state
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    data_dir: PathBuf,
}

impl Db {
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("database mutex poisoned")
    }

    /// Path to the session body storage directory
    pub fn bodies_dir(&self) -> PathBuf {
        self.data_dir.join("bodies")
    }

    /// Write a session body as HAIL JSONL to disk, return the storage key
    pub fn write_body(&self, session_id: &str, data: &[u8]) -> Result<String> {
        let dir = self.bodies_dir();
        std::fs::create_dir_all(&dir)?;
        let key = format!("{session_id}.hail.jsonl");
        let path = dir.join(&key);
        std::fs::write(&path, data).context("writing session body")?;
        Ok(key)
    }

    /// Read a session body from disk
    pub fn read_body(&self, storage_key: &str) -> Result<Vec<u8>> {
        let path = self.bodies_dir().join(storage_key);
        std::fs::read(&path).context("reading session body")
    }

    /// Check whether a user belongs to a team.
    pub fn is_team_member(&self, team_id: &str, user_id: &str) -> bool {
        let (sql, values) = db::teams::member_exists(team_id, user_id);
        sq_query_row(&self.conn(), (sql, values), |row| {
            row.get::<_, i64>(0).map(|c| c > 0)
        })
        .unwrap_or(false)
    }
}

// ── sea-query ↔ rusqlite helpers ──────────────────────────────────────────

/// Built query: `(sql, sea_query::Values)`.
pub type Built = (String, sea_query::Values);

/// Convert `sea_query::Values` to boxed rusqlite params.
pub fn sq_params(values: &sea_query::Values) -> Vec<Box<dyn rusqlite::types::ToSql>> {
    values
        .0
        .iter()
        .map(|v| -> Box<dyn rusqlite::types::ToSql> {
            match v {
                sea_query::Value::Bool(Some(b)) => Box::new(*b),
                sea_query::Value::Int(Some(i)) => Box::new(*i),
                sea_query::Value::BigInt(Some(i)) => Box::new(*i),
                sea_query::Value::String(Some(s)) => Box::new(s.as_ref().clone()),
                sea_query::Value::Bytes(Some(b)) => Box::new(b.as_ref().clone()),
                sea_query::Value::Double(Some(f)) => Box::new(*f),
                _ => Box::new(rusqlite::types::Null),
            }
        })
        .collect()
}

/// Execute a built query (INSERT/UPDATE/DELETE).
pub fn sq_execute(conn: &Connection, (sql, values): Built) -> rusqlite::Result<usize> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, refs.as_slice())
}

/// Query a single row from a built query.
pub fn sq_query_row<T>(
    conn: &Connection,
    (sql, values): Built,
    f: impl FnOnce(&rusqlite::Row) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&sql, refs.as_slice(), f)
}

/// Prepare + query_map from a built query, collecting into a Vec.
pub fn sq_query_map<T>(
    conn: &Connection,
    (sql, values): Built,
    f: impl FnMut(&rusqlite::Row) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    let params = sq_params(&values);
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(refs.as_slice(), f)?;
    rows.collect()
}

// ── Row mappers ───────────────────────────────────────────────────────────

/// Map a `session_columns()` row into a `SessionSummary`.
pub fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        user_id: row.get(1)?,
        nickname: row.get(2)?,
        team_id: row.get(3)?,
        tool: row.get(4)?,
        agent_provider: row.get(5)?,
        agent_model: row.get(6)?,
        title: row.get(7)?,
        description: row.get(8)?,
        tags: row.get(9)?,
        created_at: row.get(10)?,
        uploaded_at: row.get(11)?,
        message_count: row.get(12)?,
        task_count: row.get(13)?,
        event_count: row.get(14)?,
        duration_seconds: row.get(15)?,
        total_input_tokens: row.get(16)?,
        total_output_tokens: row.get(17)?,
        git_remote: row.get(18)?,
        git_branch: row.get(19)?,
        git_commit: row.get(20)?,
        git_repo_name: row.get(21)?,
        pr_number: row.get(22)?,
        pr_url: row.get(23)?,
        working_directory: row.get(24)?,
        files_modified: row.get(25)?,
        files_read: row.get(26)?,
        has_errors: row.get::<_, i64>(27).unwrap_or(0) != 0,
    })
}

/// Map a `team_columns()` row into a `TeamResponse`.
pub fn team_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TeamResponse> {
    Ok(TeamResponse {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        is_public: row.get(3)?,
        created_by: row.get(4)?,
        created_at: row.get(5)?,
    })
}

// ── Database init ─────────────────────────────────────────────────────────

/// Initialize the database: open connection, enable WAL, run migrations
pub fn init_db(data_dir: &Path) -> Result<Db> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("opensession.db");
    let conn = Connection::open(&db_path).context("opening SQLite database")?;

    // Enable WAL mode for better concurrent read performance
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    run_migrations(&conn)?;

    Ok(Db {
        conn: Arc::new(Mutex::new(conn)),
        data_dir: data_dir.to_path_buf(),
    })
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    for (name, sql) in db::migrations::MIGRATIONS {
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !already_applied {
            conn.execute_batch(sql)
                .with_context(|| format!("running migration {name}"))?;
            conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [name])?;
            tracing::info!("Applied migration: {name}");
        }
    }

    Ok(())
}
