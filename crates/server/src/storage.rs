use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
        self.conn()
            .query_row(
                "SELECT COUNT(*) > 0 FROM team_members WHERE team_id = ?1 AND user_id = ?2",
                rusqlite::params![team_id, user_id],
                |row| row.get(0),
            )
            .unwrap_or(false)
    }
}

/// Map a `SESSION_COLUMNS` row into a `SessionSummary`.
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
    })
}

/// Map a `TEAM_COLUMNS` row into a `TeamResponse`.
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

    let migrations = vec![
        (
            "0001_init",
            include_str!("../../../migrations/0001_init.sql"),
        ),
        (
            "0002_add_tokens_and_public",
            include_str!("../../../migrations/0002_add_tokens_and_public.sql"),
        ),
        (
            "0003_session_links",
            include_str!("../../../migrations/0003_session_links.sql"),
        ),
        (
            "0004_auth",
            include_str!("../../../migrations/0004_auth.sql"),
        ),
        (
            "0005_invitations",
            include_str!("../../../migrations/0005_invitations.sql"),
        ),
    ];

    for (name, sql) in migrations {
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
