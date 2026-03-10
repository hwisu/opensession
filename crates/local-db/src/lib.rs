pub mod git;

mod connection;
mod job_store;
mod migrations;
mod repo_store;
mod session_store;
mod summary_store;
mod sync_store;
mod vector_store;

pub use connection::LocalDb;
pub use job_store::{LifecycleCleanupJobRow, SummaryBatchJobRow, VectorIndexJobRow};
pub use session_store::{
    LocalSessionFilter, LocalSessionLink, LocalSessionRow, LocalSortOrder, LocalTimeRange,
    LogFilter, RemoteSessionSummary,
};
pub use summary_store::{SessionSemanticSummaryRow, SessionSemanticSummaryUpsert};
pub use vector_store::{VectorChunkCandidateRow, VectorChunkUpsert};

#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use opensession_api::db::migrations::LOCAL_MIGRATIONS;
#[cfg(test)]
use opensession_core::trace::Session;
#[cfg(test)]
use rusqlite::{Connection, params};
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use vector_store::build_fts_query;

#[cfg(test)]
mod tests {
    use super::*;

    use opensession_api::{
        JobManifest, JobProtocol, JobReviewKind, JobStage, JobStatus, apply_job_manifest,
    };
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    fn test_db() -> LocalDb {
        let dir = tempdir().unwrap();
        let path = dir.keep().join("test.db");
        LocalDb::open_path(&path).unwrap()
    }

    #[test]
    fn test_open_and_schema() {
        let _db = test_db();
    }

    #[test]
    fn test_open_repairs_codex_tool_hint_from_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair.db");

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key) VALUES (?1, 'personal', 'claude-code', ?2, '')",
                params!["rollout-repair", "2026-02-20T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-repair", "/Users/test/.codex/sessions/2026/02/20/rollout-repair.jsonl"],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "rollout-repair")
            .expect("repaired row");
        assert_eq!(row.tool, "codex");
    }

    #[test]
    fn test_open_repairs_codex_auxiliary_flag_from_source_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair-auxiliary.db");
        let codex_dir = dir
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("02")
            .join("20");
        create_dir_all(&codex_dir).unwrap();
        let source_path = codex_dir.join("rollout-subagent.jsonl");
        write(
            &source_path,
            r#"{"timestamp":"2026-02-20T00:00:00.000Z","type":"session_meta","payload":{"id":"rollout-subagent","timestamp":"2026-02-20T00:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.105.0","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-session-id","depth":1,"agent_role":"awaiter"}}},"agent_role":"awaiter"}}\n"#,
        )
        .unwrap();

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key, is_auxiliary) VALUES (?1, 'personal', 'codex', ?2, '', 0)",
                params!["rollout-subagent", "2026-02-20T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-subagent", source_path.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "rollout-subagent"),
            "auxiliary codex session should be hidden after repair"
        );
    }

    #[test]
    fn test_open_repairs_codex_auxiliary_flag_when_session_meta_is_not_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair-auxiliary-shifted.db");
        let codex_dir = dir
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("03")
            .join("03");
        create_dir_all(&codex_dir).unwrap();
        let source_path = codex_dir.join("rollout-subagent-shifted.jsonl");
        write(
            &source_path,
            [
                r#"{"timestamp":"2026-03-03T00:00:00.010Z","type":"event_msg","payload":{"type":"agent_message","message":"bootstrap line"}}"#,
                r#"{"timestamp":"2026-03-03T00:00:00.020Z","type":"session_meta","payload":{"id":"rollout-subagent-shifted","timestamp":"2026-03-03T00:00:00.000Z","cwd":"/tmp","originator":"Codex Desktop","cli_version":"0.108.0","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-session-id","depth":1,"agent_role":"worker"}}},"agent_role":"worker"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        {
            let _ = LocalDb::open_path(&path).unwrap();
        }

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, team_id, tool, created_at, body_storage_key, is_auxiliary) VALUES (?1, 'personal', 'codex', ?2, '', 0)",
                params!["rollout-subagent-shifted", "2026-03-03T00:00:00Z"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_sync (session_id, source_path, sync_status) VALUES (?1, ?2, 'local_only')",
                params!["rollout-subagent-shifted", source_path.to_string_lossy().to_string()],
            )
            .unwrap();
        }

        let db = LocalDb::open_path(&path).unwrap();
        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "rollout-subagent-shifted"),
            "auxiliary codex session should be hidden after repair even if session_meta is not the first line"
        );
    }

    #[test]
    fn test_upsert_local_session_normalizes_tool_from_source_path() {
        let db = test_db();
        let mut session = Session::new(
            "rollout-upsert".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;

        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/rollout-upsert.jsonl",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "rollout-upsert")
            .expect("upserted row");
        assert_eq!(row.tool, "codex");
    }

    #[test]
    fn test_upsert_local_session_indexes_job_context() {
        let db = test_db();
        let mut session = Session::new(
            "job-indexed".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        apply_job_manifest(
            &mut session,
            &JobManifest {
                protocol: JobProtocol::AgentCommunicationProtocol,
                system: "symphony".to_string(),
                job_id: "AUTH-777".to_string(),
                job_title: "Index job context".to_string(),
                run_id: "run-7".to_string(),
                attempt: 7,
                stage: JobStage::Review,
                review_kind: Some(JobReviewKind::Todo),
                status: JobStatus::Pending,
                thread_id: Some("thread-7".to_string()),
                artifacts: vec![],
            },
        );

        db.upsert_local_session_with_storage_key(
            &session,
            "/Users/test/.codex/sessions/2026/03/10/job-indexed.jsonl",
            &crate::git::GitContext::default(),
            Some("os://src/local/test-job-indexed"),
        )
        .unwrap();

        let rows = db.list_sessions_for_job("AUTH-777").unwrap();
        assert_eq!(rows.len(), 1);
        let job_context = rows[0].job_context.clone().expect("job context");
        assert_eq!(job_context.system, "symphony");
        assert_eq!(job_context.job_id, "AUTH-777");
        assert_eq!(job_context.job_title, "Index job context");
        assert_eq!(job_context.run_id, "run-7");
        assert_eq!(job_context.attempt, 7);
        assert_eq!(job_context.stage, JobStage::Review);
        assert_eq!(job_context.review_kind, Some(JobReviewKind::Todo));
        assert_eq!(job_context.status, JobStatus::Pending);
        assert_eq!(job_context.thread_id.as_deref(), Some("thread-7"));
        assert_eq!(
            rows[0].body_storage_key.as_deref(),
            Some("os://src/local/test-job-indexed")
        );
    }

    #[test]
    fn test_upsert_local_session_preserves_existing_git_when_session_has_no_git_metadata() {
        let db = test_db();
        let mut session = Session::new(
            "preserve-git".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;

        let first_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("feature/original".to_string()),
            commit: Some("1111111".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/preserve-git.jsonl",
            &first_git,
        )
        .unwrap();

        let second_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("feature/current-head".to_string()),
            commit: Some("2222222".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.codex/sessions/2026/02/20/preserve-git.jsonl",
            &second_git,
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "preserve-git")
            .expect("row exists");
        assert_eq!(row.git_branch.as_deref(), Some("feature/original"));
        assert_eq!(row.git_commit.as_deref(), Some("1111111"));
    }

    #[test]
    fn test_upsert_local_session_prefers_git_branch_from_session_attributes() {
        let db = test_db();
        let mut session = Session::new(
            "session-git-branch".to_string(),
            opensession_core::trace::Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String("from-session".to_string()),
        );

        let fallback_git = crate::git::GitContext {
            remote: Some("https://github.com/acme/repo.git".to_string()),
            branch: Some("fallback-branch".to_string()),
            commit: Some("aaaaaaaa".to_string()),
            repo_name: Some("acme/repo".to_string()),
        };
        db.upsert_local_session(
            &session,
            "/Users/test/.claude/projects/foo/session-git-branch.jsonl",
            &fallback_git,
        )
        .unwrap();

        session.context.attributes.insert(
            "git_branch".to_string(),
            serde_json::Value::String("from-session-updated".to_string()),
        );
        db.upsert_local_session(
            &session,
            "/Users/test/.claude/projects/foo/session-git-branch.jsonl",
            &fallback_git,
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "session-git-branch")
            .expect("row exists");
        assert_eq!(row.git_branch.as_deref(), Some("from-session-updated"));
    }

    #[test]
    fn test_upsert_local_session_marks_parented_sessions_auxiliary() {
        let db = test_db();
        let mut session = Session::new(
            "aux-upsert".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "opencode".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            opensession_core::session::ATTR_PARENT_SESSION_ID.to_string(),
            serde_json::Value::String("parent-session".to_string()),
        );

        db.upsert_local_session(
            &session,
            "/Users/test/.opencode/storage/session/project/aux-upsert.json",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let is_auxiliary: i64 = db
            .conn()
            .query_row(
                "SELECT is_auxiliary FROM sessions WHERE id = ?1",
                params!["aux-upsert"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_auxiliary, 1);

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "aux-upsert"),
            "auxiliary sessions should be hidden from default listing"
        );
    }

    #[test]
    fn test_upsert_local_session_primary_role_overrides_parent_link() {
        let db = test_db();
        let mut session = Session::new(
            "primary-override".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "opencode".to_string(),
                tool_version: None,
            },
        );
        session.stats.event_count = 1;
        session.context.attributes.insert(
            opensession_core::session::ATTR_PARENT_SESSION_ID.to_string(),
            serde_json::Value::String("parent-session".to_string()),
        );
        session.context.attributes.insert(
            opensession_core::session::ATTR_SESSION_ROLE.to_string(),
            serde_json::Value::String("primary".to_string()),
        );

        db.upsert_local_session(
            &session,
            "/Users/test/.opencode/storage/session/project/primary-override.json",
            &crate::git::GitContext::default(),
        )
        .unwrap();

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.id == "primary-override")
            .expect("session with explicit primary role should stay visible");
        assert!(!row.is_auxiliary);
    }

    #[test]
    fn test_upsert_local_session_skips_empty_signal_rows() {
        let db = test_db();
        let session = Session::new(
            "empty-signal-local".to_string(),
            opensession_core::trace::Agent {
                provider: "sourcegraph".to_string(),
                model: "amp-model".to_string(),
                tool: "amp".to_string(),
                tool_version: None,
            },
        );

        db.upsert_local_session(
            &session,
            "/Users/test/.local/share/amp/threads/T-empty-signal-local.json",
            &crate::git::GitContext::default(),
        )
        .expect("upsert empty-signal session should not fail");

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "empty-signal-local"),
            "empty-signal local sessions must not be listed",
        );
    }

    #[test]
    fn test_upsert_local_session_empty_signal_deletes_existing_row() {
        let db = test_db();
        let mut populated = Session::new(
            "empty-signal-replace".to_string(),
            opensession_core::trace::Agent {
                provider: "sourcegraph".to_string(),
                model: "amp-model".to_string(),
                tool: "amp".to_string(),
                tool_version: None,
            },
        );
        populated.stats.event_count = 2;
        populated.stats.message_count = 1;
        populated.stats.user_message_count = 1;

        db.upsert_local_session(
            &populated,
            "/Users/test/.local/share/amp/threads/T-empty-signal-replace.json",
            &crate::git::GitContext::default(),
        )
        .expect("seed populated row");
        assert!(
            db.get_session_by_id("empty-signal-replace")
                .unwrap()
                .is_some()
        );

        let empty = Session::new(
            "empty-signal-replace".to_string(),
            opensession_core::trace::Agent {
                provider: "sourcegraph".to_string(),
                model: "amp-model".to_string(),
                tool: "amp".to_string(),
                tool_version: None,
            },
        );
        db.upsert_local_session(
            &empty,
            "/Users/test/.local/share/amp/threads/T-empty-signal-replace.json",
            &crate::git::GitContext::default(),
        )
        .expect("upsert empty-signal replacement");

        assert!(
            db.get_session_by_id("empty-signal-replace")
                .unwrap()
                .is_none(),
            "existing local row must be removed when source becomes empty-signal",
        );
    }

    #[test]
    fn test_list_sessions_hides_codex_summary_worker_titles() {
        let db = test_db();
        let mut codex_summary_worker = Session::new(
            "codex-summary-worker".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        codex_summary_worker.context.title = Some(
            "Convert a real coding session into semantic compression. Pipeline: ...".to_string(),
        );
        codex_summary_worker.stats.event_count = 2;
        codex_summary_worker.stats.message_count = 1;

        db.upsert_local_session(
            &codex_summary_worker,
            "/Users/test/.codex/sessions/2026/03/05/summary-worker.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert codex summary worker session");

        let mut non_codex_same_title = Session::new(
            "claude-similar-title".to_string(),
            opensession_core::trace::Agent {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-6".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        non_codex_same_title.context.title = Some(
            "Convert a real coding session into semantic compression. Pipeline: ...".to_string(),
        );
        non_codex_same_title.stats.event_count = 2;
        non_codex_same_title.stats.message_count = 1;

        db.upsert_local_session(
            &non_codex_same_title,
            "/Users/test/.claude/projects/p1/claude-similar-title.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert non-codex session");

        let rows = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert!(
            rows.iter().all(|row| row.id != "codex-summary-worker"),
            "codex summary worker sessions should be hidden from default listing"
        );
        assert!(
            rows.iter().any(|row| row.id == "claude-similar-title"),
            "non-codex sessions must remain visible even with similar title"
        );

        let count = db
            .count_sessions_filtered(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sync_cursor() {
        let db = test_db();
        assert_eq!(db.get_sync_cursor("team1").unwrap(), None);
        db.set_sync_cursor("team1", "2024-01-01T00:00:00Z").unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-01-01T00:00:00Z".to_string())
        );
        // Update
        db.set_sync_cursor("team1", "2024-06-01T00:00:00Z").unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-06-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_list_session_source_paths_returns_non_empty_paths_only() {
        let db = test_db();
        let mut s1 = Session::new(
            "source-path-1".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        s1.stats.event_count = 1;
        db.upsert_local_session(
            &s1,
            "/tmp/source-path-1.jsonl",
            &crate::git::GitContext::default(),
        )
        .expect("upsert first session");

        let mut s2 = Session::new(
            "source-path-2".to_string(),
            opensession_core::trace::Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        s2.stats.event_count = 1;
        db.upsert_local_session(&s2, "", &crate::git::GitContext::default())
            .expect("upsert second session");

        let paths = db
            .list_session_source_paths()
            .expect("list source paths should work");
        assert!(
            paths
                .iter()
                .any(|(id, path)| id == "source-path-1" && path == "/tmp/source-path-1.jsonl")
        );
        assert!(paths.iter().all(|(id, _)| id != "source-path-2"));
    }

    #[test]
    fn test_body_cache() {
        let db = test_db();
        assert_eq!(db.get_cached_body("s1").unwrap(), None);
        db.cache_body("s1", b"hello world").unwrap();
        assert_eq!(
            db.get_cached_body("s1").unwrap(),
            Some(b"hello world".to_vec())
        );
    }

    #[test]
    fn test_get_session_by_id_and_list_session_links() {
        let db = test_db();
        db.upsert_remote_session(&make_summary(
            "parent-session",
            "codex",
            "Parent session",
            "2024-01-01T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "child-session",
            "codex",
            "Child session",
            "2024-01-01T01:00:00Z",
        ))
        .unwrap();

        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) VALUES (?1, ?2, ?3, ?4)",
                params!["parent-session", "child-session", "handoff", "2024-01-01T01:00:00Z"],
            )
            .unwrap();

        let parent = db
            .get_session_by_id("parent-session")
            .unwrap()
            .expect("session should exist");
        assert_eq!(parent.id, "parent-session");
        assert_eq!(parent.title.as_deref(), Some("Parent session"));

        let links = db.list_session_links("parent-session").unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].session_id, "parent-session");
        assert_eq!(links[0].linked_session_id, "child-session");
        assert_eq!(links[0].link_type, "handoff");
    }

    #[test]
    fn test_local_migrations_are_loaded_from_api_crate() {
        let migration_names: Vec<&str> = super::LOCAL_MIGRATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert!(
            migration_names.contains(&"local_0001_schema"),
            "expected local_0001_schema migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0002_session_summaries"),
            "expected local_0002_session_summaries migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0003_vector_index"),
            "expected local_0003_vector_index migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0004_summary_batch_status"),
            "expected local_0004_summary_batch_status migration from opensession-api"
        );
        assert!(
            migration_names.contains(&"local_0005_lifecycle_cleanup_status"),
            "expected local_0005_lifecycle_cleanup_status migration from opensession-api"
        );
        assert_eq!(
            migration_names.len(),
            5,
            "local schema should include baseline + summary cache + vector index + summary batch status + lifecycle cleanup status steps"
        );

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let migrations_dir = manifest_dir.join("migrations");
        if migrations_dir.exists() {
            let sql_files = std::fs::read_dir(migrations_dir)
                .expect("read local-db migrations directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.ends_with(".sql"))
                .collect::<Vec<_>>();
            assert!(
                sql_files.is_empty(),
                "local-db must not ship duplicated migration SQL files"
            );
        }
    }

    #[test]
    fn test_local_schema_bootstrap_includes_is_auxiliary_column() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local.db");
        let db = LocalDb::open_path(&path).unwrap();
        let conn = db.conn();
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            columns.iter().any(|name| name == "is_auxiliary"),
            "sessions schema must include is_auxiliary column in bootstrap migration"
        );
    }

    #[test]
    fn test_upsert_remote_session() {
        let db = test_db();
        let summary = RemoteSessionSummary {
            id: "remote-1".to_string(),
            user_id: Some("u1".to_string()),
            nickname: Some("alice".to_string()),
            team_id: "t1".to_string(),
            tool: "claude-code".to_string(),
            agent_provider: None,
            agent_model: None,
            title: Some("Test session".to_string()),
            description: None,
            tags: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            uploaded_at: "2024-01-01T01:00:00Z".to_string(),
            message_count: 10,
            task_count: 2,
            event_count: 20,
            duration_seconds: 300,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            job_context: None,
        };
        db.upsert_remote_session(&summary).unwrap();

        let sessions = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "remote-1");
        assert_eq!(sessions[0].sync_status, "remote_only");
        assert_eq!(sessions[0].nickname, None); // no user in local users table
        assert!(!sessions[0].is_auxiliary);
    }

    #[test]
    fn test_list_filter_by_repo() {
        let db = test_db();
        // Insert a remote session with team_id
        let summary1 = RemoteSessionSummary {
            id: "s1".to_string(),
            user_id: None,
            nickname: None,
            team_id: "t1".to_string(),
            tool: "claude-code".to_string(),
            agent_provider: None,
            agent_model: None,
            title: Some("Session 1".to_string()),
            description: None,
            tags: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            uploaded_at: "2024-01-01T01:00:00Z".to_string(),
            message_count: 5,
            task_count: 0,
            event_count: 10,
            duration_seconds: 60,
            total_input_tokens: 100,
            total_output_tokens: 50,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            job_context: None,
        };
        db.upsert_remote_session(&summary1).unwrap();

        // Filter by team
        let filter = LocalSessionFilter {
            team_id: Some("t1".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&filter).unwrap().len(), 1);

        let filter = LocalSessionFilter {
            team_id: Some("t999".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&filter).unwrap().len(), 0);
    }

    // ── Helpers for inserting test sessions ────────────────────────────

    fn make_summary(id: &str, tool: &str, title: &str, created_at: &str) -> RemoteSessionSummary {
        RemoteSessionSummary {
            id: id.to_string(),
            user_id: None,
            nickname: None,
            team_id: "t1".to_string(),
            tool: tool.to_string(),
            agent_provider: Some("anthropic".to_string()),
            agent_model: Some("claude-opus-4-6".to_string()),
            title: Some(title.to_string()),
            description: None,
            tags: None,
            created_at: created_at.to_string(),
            uploaded_at: created_at.to_string(),
            message_count: 5,
            task_count: 1,
            event_count: 10,
            duration_seconds: 300,
            total_input_tokens: 1000,
            total_output_tokens: 500,
            git_remote: None,
            git_branch: None,
            git_commit: None,
            git_repo_name: None,
            pr_number: None,
            pr_url: None,
            working_directory: None,
            files_modified: None,
            files_read: None,
            has_errors: false,
            max_active_agents: 1,
            job_context: None,
        }
    }

    fn seed_sessions(db: &LocalDb) {
        // Insert 5 sessions across two tools, ordered by created_at
        db.upsert_remote_session(&make_summary(
            "s1",
            "claude-code",
            "First session",
            "2024-01-01T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s2",
            "claude-code",
            "JWT auth work",
            "2024-01-02T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s3",
            "gemini",
            "Gemini test",
            "2024-01-03T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s4",
            "claude-code",
            "Error handling",
            "2024-01-04T00:00:00Z",
        ))
        .unwrap();
        db.upsert_remote_session(&make_summary(
            "s5",
            "claude-code",
            "Final polish",
            "2024-01-05T00:00:00Z",
        ))
        .unwrap();
    }

    // ── list_sessions_log tests ────────────────────────────────────────

    #[test]
    fn test_log_no_filters() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter::default();
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 5);
        // Should be ordered by created_at DESC
        assert_eq!(results[0].id, "s5");
        assert_eq!(results[4].id, "s1");
    }

    #[test]
    fn test_log_filter_by_tool() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            tool: Some("claude-code".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|s| s.tool == "claude-code"));
    }

    #[test]
    fn test_log_filter_by_model_wildcard() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            model: Some("claude*".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 5); // all have claude-opus model
    }

    #[test]
    fn test_log_filter_since() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            since: Some("2024-01-03T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 3); // s3, s4, s5
    }

    #[test]
    fn test_log_filter_before() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            before: Some("2024-01-03T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2); // s1, s2
    }

    #[test]
    fn test_log_filter_since_and_before() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            since: Some("2024-01-02T00:00:00Z".to_string()),
            before: Some("2024-01-04T00:00:00Z".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2); // s2, s3
    }

    #[test]
    fn test_log_filter_grep() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            grep: Some("JWT".to_string()),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s2");
    }

    #[test]
    fn test_log_limit_and_offset() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "s4"); // second most recent
        assert_eq!(results[1].id, "s3");
    }

    #[test]
    fn test_log_limit_only() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            limit: Some(3),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list_sessions_limit_offset() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LocalSessionFilter {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "s4");
        assert_eq!(results[1].id, "s3");
    }

    #[test]
    fn test_count_sessions_filtered() {
        let db = test_db();
        seed_sessions(&db);
        let count = db
            .count_sessions_filtered(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_list_and_count_filters_match_when_auxiliary_rows_exist() {
        let db = test_db();
        seed_sessions(&db);
        db.conn()
            .execute(
                "UPDATE sessions SET is_auxiliary = 1 WHERE id IN ('s2', 's3')",
                [],
            )
            .unwrap();

        let default_filter = LocalSessionFilter::default();
        let rows = db.list_sessions(&default_filter).unwrap();
        let count = db.count_sessions_filtered(&default_filter).unwrap();
        assert_eq!(rows.len() as i64, count);
        assert!(rows.iter().all(|row| !row.is_auxiliary));

        let gemini_filter = LocalSessionFilter {
            tool: Some("gemini".to_string()),
            ..Default::default()
        };
        let gemini_rows = db.list_sessions(&gemini_filter).unwrap();
        let gemini_count = db.count_sessions_filtered(&gemini_filter).unwrap();
        assert_eq!(gemini_rows.len() as i64, gemini_count);
        assert!(gemini_rows.is_empty());
        assert_eq!(gemini_count, 0);
    }

    #[test]
    fn test_exclude_low_signal_filter_hides_metadata_only_sessions() {
        let db = test_db();

        let mut low_signal = make_summary("meta-only", "claude-code", "", "2024-01-01T00:00:00Z");
        low_signal.title = None;
        low_signal.message_count = 0;
        low_signal.task_count = 0;
        low_signal.event_count = 2;
        low_signal.git_repo_name = Some("frontend/aviss-react-front".to_string());

        let mut normal = make_summary(
            "real-work",
            "opencode",
            "Socket.IO decision",
            "2024-01-02T00:00:00Z",
        );
        normal.message_count = 14;
        normal.task_count = 2;
        normal.event_count = 38;
        normal.git_repo_name = Some("frontend/aviss-react-front".to_string());

        db.upsert_remote_session(&low_signal).unwrap();
        db.upsert_remote_session(&normal).unwrap();

        let default_filter = LocalSessionFilter {
            git_repo_name: Some("frontend/aviss-react-front".to_string()),
            ..Default::default()
        };
        assert_eq!(db.list_sessions(&default_filter).unwrap().len(), 2);
        assert_eq!(db.count_sessions_filtered(&default_filter).unwrap(), 2);

        let repo_filter = LocalSessionFilter {
            git_repo_name: Some("frontend/aviss-react-front".to_string()),
            exclude_low_signal: true,
            ..Default::default()
        };
        let rows = db.list_sessions(&repo_filter).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "real-work");
        assert_eq!(db.count_sessions_filtered(&repo_filter).unwrap(), 1);
    }

    #[test]
    fn test_list_working_directories_distinct_non_empty() {
        let db = test_db();

        let mut a = make_summary("wd-1", "claude-code", "One", "2024-01-01T00:00:00Z");
        a.working_directory = Some("/tmp/repo-a".to_string());
        let mut b = make_summary("wd-2", "claude-code", "Two", "2024-01-02T00:00:00Z");
        b.working_directory = Some("/tmp/repo-a".to_string());
        let mut c = make_summary("wd-3", "claude-code", "Three", "2024-01-03T00:00:00Z");
        c.working_directory = Some("/tmp/repo-b".to_string());
        let mut d = make_summary("wd-4", "claude-code", "Four", "2024-01-04T00:00:00Z");
        d.working_directory = Some("".to_string());

        db.upsert_remote_session(&a).unwrap();
        db.upsert_remote_session(&b).unwrap();
        db.upsert_remote_session(&c).unwrap();
        db.upsert_remote_session(&d).unwrap();

        let dirs = db.list_working_directories().unwrap();
        assert_eq!(
            dirs,
            vec!["/tmp/repo-a".to_string(), "/tmp/repo-b".to_string()]
        );
    }

    #[test]
    fn test_list_session_tools() {
        let db = test_db();
        seed_sessions(&db);
        let tools = db
            .list_session_tools(&LocalSessionFilter::default())
            .unwrap();
        assert_eq!(tools, vec!["claude-code".to_string(), "gemini".to_string()]);
    }

    #[test]
    fn test_log_combined_filters() {
        let db = test_db();
        seed_sessions(&db);
        let filter = LogFilter {
            tool: Some("claude-code".to_string()),
            since: Some("2024-01-03T00:00:00Z".to_string()),
            limit: Some(1),
            ..Default::default()
        };
        let results = db.list_sessions_log(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s5"); // most recent claude-code after Jan 3
    }

    // ── Session offset/latest tests ────────────────────────────────────

    #[test]
    fn test_get_session_by_offset() {
        let db = test_db();
        seed_sessions(&db);
        let row = db.get_session_by_offset(0).unwrap().unwrap();
        assert_eq!(row.id, "s5"); // most recent
        let row = db.get_session_by_offset(2).unwrap().unwrap();
        assert_eq!(row.id, "s3");
        assert!(db.get_session_by_offset(10).unwrap().is_none());
    }

    #[test]
    fn test_get_session_by_tool_offset() {
        let db = test_db();
        seed_sessions(&db);
        let row = db
            .get_session_by_tool_offset("claude-code", 0)
            .unwrap()
            .unwrap();
        assert_eq!(row.id, "s5");
        let row = db
            .get_session_by_tool_offset("claude-code", 1)
            .unwrap()
            .unwrap();
        assert_eq!(row.id, "s4");
        let row = db.get_session_by_tool_offset("gemini", 0).unwrap().unwrap();
        assert_eq!(row.id, "s3");
        assert!(
            db.get_session_by_tool_offset("gemini", 1)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_get_sessions_latest() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_latest(3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "s5");
        assert_eq!(rows[1].id, "s4");
        assert_eq!(rows[2].id, "s3");
    }

    #[test]
    fn test_get_sessions_by_tool_latest() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_by_tool_latest("claude-code", 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "s5");
        assert_eq!(rows[1].id, "s4");
    }

    #[test]
    fn test_get_sessions_latest_more_than_available() {
        let db = test_db();
        seed_sessions(&db);
        let rows = db.get_sessions_by_tool_latest("gemini", 10).unwrap();
        assert_eq!(rows.len(), 1); // only 1 gemini session
    }

    #[test]
    fn test_upsert_and_get_session_semantic_summary() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"updated files","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("abc123"),
            source_details_json: Some(r#"{"source":"session"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("upsert semantic summary");

        let row = db
            .get_session_semantic_summary("s1")
            .expect("query semantic summary")
            .expect("summary row exists");
        assert_eq!(row.session_id, "s1");
        assert_eq!(row.provider, "codex_exec");
        assert_eq!(row.model.as_deref(), Some("gpt-5"));
        assert_eq!(row.source_kind, "session_signals");
        assert_eq!(row.generation_kind, "provider");
        assert_eq!(row.prompt_fingerprint.as_deref(), Some("abc123"));
        assert!(row.error.is_none());
    }

    #[test]
    fn test_delete_session_removes_semantic_summary_row() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"updated files","auth_security":"none detected","layer_file_changes":[]}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "heuristic",
            model: None,
            source_kind: "heuristic",
            generation_kind: "heuristic_fallback",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: Some("provider disabled"),
        })
        .expect("upsert semantic summary");

        db.delete_session("s1").expect("delete session");

        let missing = db
            .get_session_semantic_summary("s1")
            .expect("query semantic summary");
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_session_removes_session_links_bidirectionally() {
        let db = test_db();
        seed_sessions(&db);

        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) \
                 VALUES (?1, ?2, 'handoff', datetime('now'))",
                params!["s1", "s2"],
            )
            .expect("insert forward link");
        db.conn()
            .execute(
                "INSERT INTO session_links (session_id, linked_session_id, link_type, created_at) \
                 VALUES (?1, ?2, 'related', datetime('now'))",
                params!["s3", "s1"],
            )
            .expect("insert reverse link");

        db.delete_session("s1").expect("delete root session");

        let remaining: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM session_links WHERE session_id = ?1 OR linked_session_id = ?1",
                params!["s1"],
                |row| row.get(0),
            )
            .expect("count linked rows");
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_delete_expired_session_summaries_uses_generated_at_ttl() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s1",
            summary_json: r#"{"changes":"old"}"#,
            generated_at: "2020-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("upsert old summary");
        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s2",
            summary_json: r#"{"changes":"new"}"#,
            generated_at: "2999-01-01T00:00:00Z",
            provider: "codex_exec",
            model: None,
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: None,
            source_details_json: None,
            diff_tree_json: None,
            error: None,
        })
        .expect("upsert new summary");

        let deleted = db
            .delete_expired_session_summaries(30)
            .expect("delete expired summaries");
        assert_eq!(deleted, 1);
        assert!(
            db.get_session_semantic_summary("s1")
                .expect("query old summary")
                .is_none()
        );
        assert!(
            db.get_session_semantic_summary("s2")
                .expect("query new summary")
                .is_some()
        );
    }

    #[test]
    fn test_list_all_session_ids_returns_sorted_ids() {
        let db = test_db();
        seed_sessions(&db);

        let ids = db.list_all_session_ids().expect("list all session ids");
        assert_eq!(ids, vec!["s1", "s2", "s3", "s4", "s5"]);
    }

    #[test]
    fn test_list_session_semantic_summary_ids_returns_sorted_ids() {
        let db = test_db();
        seed_sessions(&db);

        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s4",
            summary_json: r#"{"changes":"delta"}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("fingerprint"),
            source_details_json: Some(r#"{"source":"session"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("upsert summary s4");
        db.upsert_session_semantic_summary(&SessionSemanticSummaryUpsert {
            session_id: "s2",
            summary_json: r#"{"changes":"delta"}"#,
            generated_at: "2026-03-04T10:00:00Z",
            provider: "codex_exec",
            model: Some("gpt-5"),
            source_kind: "session_signals",
            generation_kind: "provider",
            prompt_fingerprint: Some("fingerprint"),
            source_details_json: Some(r#"{"source":"session"}"#),
            diff_tree_json: Some(r#"[]"#),
            error: None,
        })
        .expect("upsert summary s2");

        let ids = db
            .list_session_semantic_summary_ids()
            .expect("list semantic summary ids");
        assert_eq!(ids, vec!["s2", "s4"]);
    }

    #[test]
    fn test_list_expired_session_ids_uses_created_at_ttl() {
        let db = test_db();
        seed_sessions(&db);

        let expired = db
            .list_expired_session_ids(30)
            .expect("list expired sessions");
        assert!(
            expired.contains(&"s1".to_string()),
            "older seeded sessions should be expired for 30-day keep"
        );

        let none_expired = db
            .list_expired_session_ids(10_000)
            .expect("list non-expired sessions");
        assert!(
            none_expired.is_empty(),
            "seeded sessions should be retained with a large keep window"
        );
    }

    #[test]
    fn test_build_fts_query_quotes_tokens() {
        assert_eq!(
            build_fts_query("parser retry"),
            Some("\"parser\" OR \"retry\"".to_string())
        );
        assert!(build_fts_query("   ").is_none());
    }

    #[test]
    fn test_vector_chunk_replace_and_candidate_query() {
        let db = test_db();
        seed_sessions(&db);

        let chunks = vec![
            VectorChunkUpsert {
                chunk_id: "chunk-s1-0".to_string(),
                session_id: "s1".to_string(),
                chunk_index: 0,
                start_line: 1,
                end_line: 8,
                line_count: 8,
                content: "parser selection retry after auth error".to_string(),
                content_hash: "hash-0".to_string(),
                embedding: vec![0.1, 0.2, 0.3],
            },
            VectorChunkUpsert {
                chunk_id: "chunk-s1-1".to_string(),
                session_id: "s1".to_string(),
                chunk_index: 1,
                start_line: 9,
                end_line: 15,
                line_count: 7,
                content: "session list refresh control wired to runtime".to_string(),
                content_hash: "hash-1".to_string(),
                embedding: vec![0.3, 0.2, 0.1],
            },
        ];

        db.replace_session_vector_chunks("s1", "source-hash-s1", "bge-m3", &chunks)
            .expect("replace vector chunks");

        let source_hash = db
            .vector_index_source_hash("s1")
            .expect("read source hash")
            .expect("source hash should exist");
        assert_eq!(source_hash, "source-hash-s1");

        let matches = db
            .list_vector_chunk_candidates("parser retry", "bge-m3", 10)
            .expect("query vector chunk candidates");
        assert!(
            !matches.is_empty(),
            "vector FTS query should return at least one candidate"
        );
        assert_eq!(matches[0].session_id, "s1");
        assert!(matches[0].content.contains("parser"));
    }

    #[test]
    fn test_delete_session_removes_vector_index_rows() {
        let db = test_db();
        seed_sessions(&db);

        let chunks = vec![VectorChunkUpsert {
            chunk_id: "chunk-s1-delete".to_string(),
            session_id: "s1".to_string(),
            chunk_index: 0,
            start_line: 1,
            end_line: 2,
            line_count: 2,
            content: "delete me from vector cache".to_string(),
            content_hash: "hash-delete".to_string(),
            embedding: vec![0.7, 0.1, 0.2],
        }];
        db.replace_session_vector_chunks("s1", "delete-hash", "bge-m3", &chunks)
            .expect("insert vector chunk");

        db.delete_session("s1")
            .expect("delete session with vector rows");

        let candidates = db
            .list_vector_chunk_candidates("delete", "bge-m3", 10)
            .expect("query candidates after delete");
        assert!(
            candidates.iter().all(|row| row.session_id != "s1"),
            "vector rows for deleted session should be removed"
        );
    }

    #[test]
    fn test_vector_index_job_round_trip() {
        let db = test_db();
        let payload = VectorIndexJobRow {
            status: "running".to_string(),
            processed_sessions: 2,
            total_sessions: 10,
            message: Some("indexing".to_string()),
            started_at: Some("2026-03-05T10:00:00Z".to_string()),
            finished_at: None,
        };
        db.set_vector_index_job(&payload)
            .expect("set vector index job snapshot");

        let loaded = db
            .get_vector_index_job()
            .expect("read vector index job snapshot")
            .expect("vector index job row should exist");
        assert_eq!(loaded.status, "running");
        assert_eq!(loaded.processed_sessions, 2);
        assert_eq!(loaded.total_sessions, 10);
        assert_eq!(loaded.message.as_deref(), Some("indexing"));
    }

    #[test]
    fn test_summary_batch_job_round_trip() {
        let db = test_db();
        let payload = SummaryBatchJobRow {
            status: "running".to_string(),
            processed_sessions: 4,
            total_sessions: 12,
            failed_sessions: 1,
            message: Some("processing summaries".to_string()),
            started_at: Some("2026-03-05T10:00:00Z".to_string()),
            finished_at: None,
        };
        db.set_summary_batch_job(&payload)
            .expect("set summary batch job snapshot");

        let loaded = db
            .get_summary_batch_job()
            .expect("read summary batch job snapshot")
            .expect("summary batch job row should exist");
        assert_eq!(loaded.status, "running");
        assert_eq!(loaded.processed_sessions, 4);
        assert_eq!(loaded.total_sessions, 12);
        assert_eq!(loaded.failed_sessions, 1);
        assert_eq!(loaded.message.as_deref(), Some("processing summaries"));
    }

    #[test]
    fn test_lifecycle_cleanup_job_round_trip() {
        let db = test_db();
        let payload = LifecycleCleanupJobRow {
            status: "complete".to_string(),
            deleted_sessions: 3,
            deleted_summaries: 7,
            message: Some("cleanup complete".to_string()),
            started_at: Some("2026-03-06T01:00:00Z".to_string()),
            finished_at: Some("2026-03-06T01:00:04Z".to_string()),
        };
        db.set_lifecycle_cleanup_job(&payload)
            .expect("set lifecycle cleanup job snapshot");

        let loaded = db
            .get_lifecycle_cleanup_job()
            .expect("read lifecycle cleanup job snapshot")
            .expect("lifecycle cleanup row should exist");
        assert_eq!(loaded.status, "complete");
        assert_eq!(loaded.deleted_sessions, 3);
        assert_eq!(loaded.deleted_summaries, 7);
        assert_eq!(loaded.message.as_deref(), Some("cleanup complete"));
    }

    #[test]
    fn test_session_count() {
        let db = test_db();
        assert_eq!(db.session_count().unwrap(), 0);
        seed_sessions(&db);
        assert_eq!(db.session_count().unwrap(), 5);
    }
}
