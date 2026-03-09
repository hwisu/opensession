use anyhow::{Context, Result};
use opensession_paths::local_db_path;
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
    /// `~/.local/share/opensession/local.db` or `OPENSESSION_LOCAL_DB_PATH` when set.
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
    local_db_path().context("Could not determine local db path")
}

#[cfg(test)]
mod tests {
    use super::default_db_path;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests serialize environment mutation with `env_test_lock`, so process
            // environment updates do not race with other tests in this module.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                // SAFETY: tests serialize environment mutation with `env_test_lock`.
                unsafe { std::env::set_var(self.key, value) };
            } else {
                // SAFETY: tests serialize environment mutation with `env_test_lock`.
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    #[test]
    fn default_db_path_uses_centralized_location() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _guard = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", "");
        let path = default_db_path().expect("default db path");
        assert!(path.ends_with(PathBuf::from(".local/share/opensession/local.db")));
    }

    #[test]
    fn default_db_path_honors_env_override() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _guard = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", "/tmp/custom-local.db");
        assert_eq!(
            default_db_path().expect("default db path"),
            PathBuf::from("/tmp/custom-local.db")
        );
    }
}
