//! Canonical migration definitions for all targets.
//!
//! `MIGRATIONS` — remote schema (Axum server + D1 Worker).
//! `LOCAL_MIGRATIONS` — local-only schema (TUI + Daemon).

/// A named migration: `(name, sql)`.
pub type Migration = (&'static str, &'static str);

/// Remote-schema migrations (server + worker).
pub const MIGRATIONS: &[Migration] = &[(
    "0001_schema",
    include_str!("../../migrations/0001_schema.sql"),
)];

/// Local-only migrations (TUI + Daemon).
/// These run AFTER the shared MIGRATIONS to add sync-tracking tables.
pub const LOCAL_MIGRATIONS: &[Migration] = &[(
    "local_0001_schema",
    include_str!("../../migrations/local_0001_schema.sql"),
)];

#[cfg(test)]
mod tests {
    use super::{LOCAL_MIGRATIONS, MIGRATIONS};

    #[test]
    fn schema_migration_set_is_minimal() {
        assert_eq!(MIGRATIONS.len(), 1);
        assert_eq!(MIGRATIONS[0].0, "0001_schema");
        assert_eq!(LOCAL_MIGRATIONS.len(), 1);
        assert_eq!(LOCAL_MIGRATIONS[0].0, "local_0001_schema");
    }
}
