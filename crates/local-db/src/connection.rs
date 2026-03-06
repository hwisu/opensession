use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::migrations::{
    apply_local_migrations, repair_auxiliary_flags_from_source_path,
    repair_session_tools_from_source_path, validate_local_schema,
};

/// Local SQLite index/cache shared by TUI and Daemon.
/// This is not the source of truth for canonical session bodies.
/// Thread-safe: wraps the connection in a Mutex so it can be shared via `Arc<LocalDb>`.
pub struct LocalDb {
    conn: Mutex<Connection>,
}

impl LocalDb {
    /// Open (or create) the local database at the default path.
    /// `~/.local/share/opensession/local.db`
    pub fn open() -> Result<Self> {
        let path = default_db_path()?;
        Self::open_path(&path)
    }

    /// Open (or create) the local database at a specific path.
    pub fn open_path(path: &PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir for {}", path.display()))?;
        }
        let conn = open_connection_with_latest_schema(path)
            .with_context(|| format!("open local db {}", path.display()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("local db mutex poisoned")
    }
}

fn open_connection_with_latest_schema(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // Disable FK constraints for local DB (index/cache, not source of truth)
    conn.execute_batch("PRAGMA foreign_keys=OFF;")?;

    apply_local_migrations(&conn)?;
    repair_session_tools_from_source_path(&conn)?;
    repair_auxiliary_flags_from_source_path(&conn)?;
    validate_local_schema(&conn)?;

    Ok(conn)
}

fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Could not determine home directory")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("opensession")
        .join("local.db"))
}
