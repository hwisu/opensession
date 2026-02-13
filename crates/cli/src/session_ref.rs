use anyhow::{bail, Result};
use opensession_local_db::{LocalDb, LocalSessionRow};
use std::path::PathBuf;

/// A parsed session reference.
#[derive(Debug, Clone)]
pub enum SessionRef {
    /// HEAD~N — the latest N sessions (HEAD = 1, HEAD~4 = 4)
    Latest { count: u32 },
    /// HEAD^N — the Nth most recent session (single, 0-indexed: HEAD^0 = most recent)
    Single { offset: u32 },
    /// A session ID
    Id(String),
    /// A file path
    File(PathBuf),
}

impl SessionRef {
    /// Parse a session reference string.
    ///
    /// Formats:
    ///   - "HEAD" or "HEAD~1" → Latest { count: 1 }
    ///   - "HEAD~4" → Latest { count: 4 } (latest 4 sessions)
    ///   - "HEAD^3" → Single { offset: 3 } (3rd previous session, 0-indexed)
    ///   - A path to a file → File(path)
    ///   - Anything else → Id(string)
    pub fn parse(s: &str) -> Self {
        let s = s.trim();

        // HEAD alone → latest 1
        if s.eq_ignore_ascii_case("HEAD") {
            return SessionRef::Latest { count: 1 };
        }

        // HEAD~N → latest N sessions
        if let Some(rest) = s.strip_prefix("HEAD~").or_else(|| s.strip_prefix("head~")) {
            if let Ok(n) = rest.parse::<u32>() {
                return SessionRef::Latest { count: n.max(1) };
            }
        }

        // HEAD^N → single session at offset N
        if let Some(rest) = s.strip_prefix("HEAD^").or_else(|| s.strip_prefix("head^")) {
            if let Ok(n) = rest.parse::<u32>() {
                return SessionRef::Single { offset: n };
            }
        }

        // Check if it's a file path
        let path = PathBuf::from(s);
        if path.exists() {
            return SessionRef::File(path);
        }

        // Otherwise treat as session ID
        SessionRef::Id(s.to_string())
    }

    /// Resolve a SessionRef to one or more LocalSessionRows.
    pub fn resolve(&self, db: &LocalDb, tool: Option<&str>) -> Result<Vec<LocalSessionRow>> {
        match self {
            SessionRef::Latest { count } => {
                let rows = if let Some(tool) = tool {
                    db.get_sessions_by_tool_latest(tool, *count)?
                } else {
                    db.get_sessions_latest(*count)?
                };
                if rows.is_empty() {
                    let tool_msg = tool.map(|t| format!(" for tool '{t}'")).unwrap_or_default();
                    bail!("No sessions found{tool_msg}. Run `opensession index` first.");
                }
                Ok(rows)
            }
            SessionRef::Single { offset } => {
                let row = if let Some(tool) = tool {
                    db.get_session_by_tool_offset(tool, *offset)?
                } else {
                    db.get_session_by_offset(*offset)?
                };
                match row {
                    Some(r) => Ok(vec![r]),
                    None => {
                        let tool_msg = tool.map(|t| format!(" for tool '{t}'")).unwrap_or_default();
                        bail!("No session found at HEAD^{offset}{tool_msg}. Run `opensession index` first.")
                    }
                }
            }
            SessionRef::Id(id) => {
                // Search by ID prefix
                let filter = opensession_local_db::LogFilter {
                    grep: Some(id.clone()),
                    limit: Some(1),
                    ..Default::default()
                };
                let results = db.list_sessions_log(&filter)?;
                match results.into_iter().next() {
                    Some(r) => Ok(vec![r]),
                    None => bail!("No session found with ID matching '{id}'"),
                }
            }
            SessionRef::File(_) => {
                // File refs are handled separately by parsing the file directly
                bail!("File-based SessionRef should be resolved by parsing the file, not via DB")
            }
        }
    }

    /// Resolve to exactly one session (for backwards compat where single is expected).
    pub fn resolve_one(&self, db: &LocalDb, tool: Option<&str>) -> Result<LocalSessionRow> {
        let rows = self.resolve(db, tool)?;
        Ok(rows
            .into_iter()
            .next()
            .expect("resolve guarantees non-empty"))
    }
}

/// Map tool flag names to their discover tool names.
pub fn tool_flag_to_name(flag: &str) -> &str {
    match flag {
        "claude" => "claude-code",
        "gemini" => "gemini",
        "cursor" => "cursor",
        "aider" => "aider",
        "goose" => "goose",
        "codex" => "codex",
        "opencode" => "opencode",
        "cline" => "cline",
        "amp" => "amp",
        _ => flag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_head() {
        match SessionRef::parse("HEAD") {
            SessionRef::Latest { count: 1 } => {}
            other => panic!("Expected Latest {{count: 1}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_head_tilde() {
        match SessionRef::parse("HEAD~4") {
            SessionRef::Latest { count: 4 } => {}
            other => panic!("Expected Latest {{count: 4}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_head_tilde_1() {
        match SessionRef::parse("HEAD~1") {
            SessionRef::Latest { count: 1 } => {}
            other => panic!("Expected Latest {{count: 1}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_head_tilde_0_clamps_to_1() {
        match SessionRef::parse("HEAD~0") {
            SessionRef::Latest { count: 1 } => {}
            other => panic!("Expected Latest {{count: 1}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_head_caret() {
        match SessionRef::parse("HEAD^3") {
            SessionRef::Single { offset: 3 } => {}
            other => panic!("Expected Single {{offset: 3}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_head_caret_0() {
        match SessionRef::parse("HEAD^0") {
            SessionRef::Single { offset: 0 } => {}
            other => panic!("Expected Single {{offset: 0}}, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_id() {
        match SessionRef::parse("abc123") {
            SessionRef::Id(id) => assert_eq!(id, "abc123"),
            other => panic!("Expected Id, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_case_insensitive() {
        match SessionRef::parse("head~3") {
            SessionRef::Latest { count: 3 } => {}
            other => panic!("Expected Latest {{count: 3}}, got {other:?}"),
        }
        match SessionRef::parse("head^2") {
            SessionRef::Single { offset: 2 } => {}
            other => panic!("Expected Single {{offset: 2}}, got {other:?}"),
        }
    }

    // ── DB-backed resolve tests ───────────────────────────────────────

    fn make_test_db() -> (tempfile::TempDir, LocalDb) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = LocalDb::open_path(&db_path).unwrap();
        (dir, db)
    }

    fn make_test_session(id: &str, tool: &str, created_at: &str) -> opensession_core::Session {
        use opensession_core::{testing, Session};

        let mut session = Session::new(id.to_string(), testing::agent_with(tool, "opus"));
        session.context.created_at = chrono::DateTime::parse_from_rfc3339(created_at)
            .unwrap()
            .with_timezone(&chrono::Utc);
        session.context.title = Some(format!("Session {id}"));
        session
    }

    fn insert_session(db: &LocalDb, session: &opensession_core::Session) {
        let git = opensession_local_db::git::GitContext {
            remote: None,
            branch: None,
            commit: None,
            repo_name: None,
        };
        db.upsert_local_session(session, "/tmp/test.jsonl", &git)
            .unwrap();
    }

    fn seed_db(db: &LocalDb) {
        // Insert 5 sessions with different timestamps and tools
        let sessions = [
            ("s1", "claude-code", "2025-01-01T00:00:00Z"),
            ("s2", "claude-code", "2025-01-02T00:00:00Z"),
            ("s3", "cursor", "2025-01-03T00:00:00Z"),
            ("s4", "claude-code", "2025-01-04T00:00:00Z"),
            ("s5", "gemini", "2025-01-05T00:00:00Z"),
        ];
        for (id, tool, ts) in sessions {
            insert_session(db, &make_test_session(id, tool, ts));
        }
    }

    #[test]
    fn test_resolve_latest_1() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Latest { count: 1 };
        let rows = r.resolve(&db, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s5"); // Most recent
    }

    #[test]
    fn test_resolve_latest_3() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Latest { count: 3 };
        let rows = r.resolve(&db, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "s5");
        assert_eq!(rows[1].id, "s4");
        assert_eq!(rows[2].id, "s3");
    }

    #[test]
    fn test_resolve_empty_db() {
        let (_dir, db) = make_test_db();
        let r = SessionRef::Latest { count: 1 };
        let result = r.resolve(&db, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_tool_filter() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Latest { count: 10 };
        let rows = r.resolve(&db, Some("claude-code")).unwrap();
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.tool, "claude-code");
        }
    }

    #[test]
    fn test_resolve_single_offset_0() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Single { offset: 0 };
        let rows = r.resolve(&db, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s5"); // Most recent
    }

    #[test]
    fn test_resolve_single_offset_beyond() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Single { offset: 99 };
        let result = r.resolve(&db, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_one_returns_first() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Latest { count: 3 };
        let row = r.resolve_one(&db, None).unwrap();
        assert_eq!(row.id, "s5"); // First = most recent
    }

    #[test]
    fn test_resolve_one_error_on_empty() {
        let (_dir, db) = make_test_db();
        let r = SessionRef::Latest { count: 1 };
        assert!(r.resolve_one(&db, None).is_err());
    }

    #[test]
    fn test_resolve_id_not_found() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        let r = SessionRef::Id("nonexistent".to_string());
        assert!(r.resolve(&db, None).is_err());
    }

    #[test]
    fn test_resolve_id_match() {
        let (_dir, db) = make_test_db();
        seed_db(&db);
        // SessionRef::Id uses grep (searches title/description/tags)
        // Our test sessions have titles like "Session s3"
        let r = SessionRef::Id("Session s3".to_string());
        let rows = r.resolve(&db, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s3");
    }

    #[test]
    fn test_tool_flag_to_name() {
        assert_eq!(tool_flag_to_name("claude"), "claude-code");
        assert_eq!(tool_flag_to_name("gemini"), "gemini");
        assert_eq!(tool_flag_to_name("cursor"), "cursor");
        assert_eq!(tool_flag_to_name("aider"), "aider");
        assert_eq!(tool_flag_to_name("goose"), "goose");
        assert_eq!(tool_flag_to_name("codex"), "codex");
        assert_eq!(tool_flag_to_name("opencode"), "opencode");
        assert_eq!(tool_flag_to_name("cline"), "cline");
        assert_eq!(tool_flag_to_name("amp"), "amp");
        assert_eq!(tool_flag_to_name("unknown"), "unknown");
    }
}
