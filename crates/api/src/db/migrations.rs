//! Canonical migration definitions for all targets.
//!
//! `MIGRATIONS` — remote schema (Axum server + D1 Worker).
//! `LOCAL_MIGRATIONS` — local-only schema (TUI + Daemon).

/// A named migration: `(name, sql)`.
pub type Migration = (&'static str, &'static str);

/// Remote-schema migrations (server + worker).
pub const MIGRATIONS: &[Migration] = &[
    (
        "0001_schema",
        include_str!("../../migrations/0001_schema.sql"),
    ),
    (
        "0003_max_active_agents",
        include_str!("../../migrations/0003_max_active_agents.sql"),
    ),
    (
        "0004_oauth_states_provider",
        include_str!("../../migrations/0004_oauth_states_provider.sql"),
    ),
    (
        "0005_sessions_body_url_backfill",
        include_str!("../../migrations/0005_sessions_body_url_backfill.sql"),
    ),
    (
        "0006_sessions_remove_fk_constraints",
        include_str!("../../migrations/0006_sessions_remove_fk_constraints.sql"),
    ),
    (
        "0007_sessions_list_perf_indexes",
        include_str!("../../migrations/0007_sessions_list_perf_indexes.sql"),
    ),
    (
        "0009_session_score_plugin",
        include_str!("../../migrations/0009_session_score_plugin.sql"),
    ),
    (
        "0010_api_keys_issuance",
        include_str!("../../migrations/0010_api_keys_issuance.sql"),
    ),
    (
        "0011_drop_legacy_team_refs",
        include_str!("../../migrations/0011_drop_legacy_team_refs.sql"),
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
        "local_0002_drop_unused_local_sessions",
        include_str!("../../migrations/local_0002_drop_unused_local_sessions.sql"),
    ),
    (
        "local_0003_session_flags",
        include_str!("../../migrations/local_0003_session_flags.sql"),
    ),
];

#[cfg(test)]
mod tests {
    use super::MIGRATIONS;

    #[test]
    fn includes_legacy_team_cleanup_migration() {
        let migration = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "0011_drop_legacy_team_refs")
            .expect("legacy team cleanup migration must be registered");

        assert!(
            migration.1.contains("DROP TABLE IF EXISTS team_members"),
            "cleanup migration should drop legacy team tables"
        );
    }
}
