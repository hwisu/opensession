//! Canonical migration definitions for all targets.
//!
//! `MIGRATIONS` — remote schema (Axum server + D1 Worker).
//! `LOCAL_MIGRATIONS` — local-only schema (TUI + Daemon).

/// A named migration: `(name, sql)`.
pub type Migration = (&'static str, &'static str);

pub const JOB_CONTEXT_MIGRATION_NAME: &str = "0002_job_context";
pub const JOB_CONTEXT_GUARD_COLUMN: &str = "job_protocol";

/// Remote-schema migrations (server + worker).
pub const MIGRATIONS: &[Migration] = &[
    (
        "0001_schema",
        include_str!("../../migrations/0001_schema.sql"),
    ),
    (
        JOB_CONTEXT_MIGRATION_NAME,
        include_str!("../../migrations/0002_job_context.sql"),
    ),
];

/// Local-only migrations (TUI + Daemon).
/// These run AFTER the shared MIGRATIONS to add sync-tracking tables.
pub const LOCAL_MIGRATIONS: &[Migration] = &[
    (
        "local_0001_schema",
        include_str!("../../migrations/local_0001_schema.sql"),
    ),
    (
        "local_0002_session_summaries",
        include_str!("../../migrations/local_0002_session_summaries.sql"),
    ),
    (
        "local_0003_vector_index",
        include_str!("../../migrations/local_0003_vector_index.sql"),
    ),
    (
        "local_0004_summary_batch_status",
        include_str!("../../migrations/local_0004_summary_batch_status.sql"),
    ),
    (
        "local_0005_lifecycle_cleanup_status",
        include_str!("../../migrations/local_0005_lifecycle_cleanup_status.sql"),
    ),
];

#[cfg(test)]
mod tests {
    use super::{
        JOB_CONTEXT_GUARD_COLUMN, JOB_CONTEXT_MIGRATION_NAME, LOCAL_MIGRATIONS, MIGRATIONS,
    };

    #[test]
    fn schema_migration_set_is_minimal() {
        assert_eq!(MIGRATIONS.len(), 2);
        assert_eq!(MIGRATIONS[0].0, "0001_schema");
        assert_eq!(MIGRATIONS[1].0, JOB_CONTEXT_MIGRATION_NAME);
        assert_eq!(LOCAL_MIGRATIONS.len(), 5);
        assert_eq!(LOCAL_MIGRATIONS[0].0, "local_0001_schema");
        assert_eq!(LOCAL_MIGRATIONS[1].0, "local_0002_session_summaries");
        assert_eq!(LOCAL_MIGRATIONS[2].0, "local_0003_vector_index");
        assert_eq!(LOCAL_MIGRATIONS[3].0, "local_0004_summary_batch_status");
        assert_eq!(LOCAL_MIGRATIONS[4].0, "local_0005_lifecycle_cleanup_status");
    }

    #[test]
    fn bootstrap_schema_drops_legacy_user_columns() {
        let sql = MIGRATIONS[0].1;
        assert!(
            !sql.contains("api_key       TEXT NOT NULL UNIQUE"),
            "users.api_key legacy column must not be present in bootstrap schema"
        );
        assert!(
            !sql.contains("avatar_url    TEXT"),
            "users.avatar_url legacy column must not be present in bootstrap schema"
        );
    }

    #[test]
    fn bootstrap_and_follow_up_migration_include_job_columns() {
        assert!(MIGRATIONS[0].1.contains("job_protocol"));
        assert!(MIGRATIONS[0].1.contains("job_artifact_count"));
        assert!(MIGRATIONS[1].1.contains(&format!(
            "ALTER TABLE sessions ADD COLUMN {JOB_CONTEXT_GUARD_COLUMN}"
        )));
        assert!(MIGRATIONS[1].1.contains("idx_sessions_job_review_lookup"));
    }
}
