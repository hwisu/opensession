use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

    /// Write a session body JSON to disk, return the storage key
    pub fn write_body(&self, session_id: &str, json: &[u8]) -> Result<String> {
        let dir = self.bodies_dir();
        std::fs::create_dir_all(&dir)?;
        let key = format!("{session_id}.json");
        let path = dir.join(&key);
        std::fs::write(&path, json).context("writing session body")?;
        Ok(key)
    }

    /// Read a session body JSON from disk
    pub fn read_body(&self, storage_key: &str) -> Result<Vec<u8>> {
        let path = self.bodies_dir().join(storage_key);
        std::fs::read(&path).context("reading session body")
    }
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
        ("0001_init", include_str!("../../../migrations/0001_init.sql")),
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
            conn.execute(
                "INSERT INTO _migrations (name) VALUES (?1)",
                [name],
            )?;
            tracing::info!("Applied migration: {name}");
        }
    }

    Ok(())
}
