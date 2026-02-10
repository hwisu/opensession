pub mod git;

use anyhow::{Context, Result};
use opensession_api_types::SessionSummary;
use opensession_core::trace::Session;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Mutex;

use git::GitContext;

/// A local session row stored in the local SQLite database.
#[derive(Debug, Clone)]
pub struct LocalSessionRow {
    pub id: String,
    pub source_path: Option<String>,
    pub sync_status: String,
    pub last_synced_at: Option<String>,
    pub user_id: Option<String>,
    pub nickname: Option<String>,
    pub team_id: Option<String>,
    pub tool: String,
    pub agent_provider: Option<String>,
    pub agent_model: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub created_at: String,
    pub uploaded_at: Option<String>,
    pub message_count: i64,
    pub task_count: i64,
    pub event_count: i64,
    pub duration_seconds: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub git_remote: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit: Option<String>,
    pub git_repo_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub working_directory: Option<String>,
}

/// Filter for listing sessions from the local DB.
#[derive(Debug, Default)]
pub struct LocalSessionFilter {
    pub team_id: Option<String>,
    pub sync_status: Option<String>,
    pub git_repo_name: Option<String>,
    pub search: Option<String>,
    pub tool: Option<String>,
}

/// Local SQLite database shared by TUI and Daemon.
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
        let conn =
            Connection::open(path).with_context(|| format!("open db {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(opensession_api_types::db::LOCAL_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("local db mutex poisoned")
    }

    // ── Upsert local session (parsed from file) ────────────────────────

    pub fn upsert_local_session(
        &self,
        session: &Session,
        source_path: &str,
        git: &GitContext,
    ) -> Result<()> {
        let title = session.context.title.as_deref();
        let description = session.context.description.as_deref();
        let tags = if session.context.tags.is_empty() {
            None
        } else {
            Some(session.context.tags.join(","))
        };
        let created_at = session.context.created_at.to_rfc3339();
        let cwd = session
            .context
            .attributes
            .get("cwd")
            .or_else(|| session.context.attributes.get("working_directory"))
            .and_then(|v| v.as_str().map(String::from));

        self.conn().execute(
            "INSERT INTO local_sessions \
             (id, source_path, sync_status, tool, agent_provider, agent_model, \
              title, description, tags, created_at, \
              message_count, task_count, event_count, duration_seconds, \
              total_input_tokens, total_output_tokens, \
              git_remote, git_branch, git_commit, git_repo_name, working_directory) \
             VALUES (?1,?2,'local_only',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20) \
             ON CONFLICT(id) DO UPDATE SET \
              source_path=excluded.source_path, \
              tool=excluded.tool, agent_provider=excluded.agent_provider, \
              agent_model=excluded.agent_model, \
              title=excluded.title, description=excluded.description, \
              tags=excluded.tags, \
              message_count=excluded.message_count, task_count=excluded.task_count, \
              event_count=excluded.event_count, duration_seconds=excluded.duration_seconds, \
              total_input_tokens=excluded.total_input_tokens, \
              total_output_tokens=excluded.total_output_tokens, \
              git_remote=excluded.git_remote, git_branch=excluded.git_branch, \
              git_commit=excluded.git_commit, git_repo_name=excluded.git_repo_name, \
              working_directory=excluded.working_directory",
            params![
                &session.session_id,
                source_path,
                &session.agent.tool,
                &session.agent.provider,
                &session.agent.model,
                title,
                description,
                &tags,
                &created_at,
                session.stats.message_count as i64,
                session.stats.task_count as i64,
                session.stats.event_count as i64,
                session.stats.duration_seconds as i64,
                session.stats.total_input_tokens as i64,
                session.stats.total_output_tokens as i64,
                &git.remote,
                &git.branch,
                &git.commit,
                &git.repo_name,
                &cwd,
            ],
        )?;
        Ok(())
    }

    // ── Upsert remote session (from server sync pull) ──────────────────

    pub fn upsert_remote_session(&self, summary: &SessionSummary) -> Result<()> {
        self.conn().execute(
            "INSERT INTO local_sessions \
             (id, sync_status, user_id, nickname, team_id, tool, \
              agent_provider, agent_model, title, description, tags, \
              created_at, uploaded_at, \
              message_count, task_count, event_count, duration_seconds, \
              total_input_tokens, total_output_tokens) \
             VALUES (?1,'remote_only',?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) \
             ON CONFLICT(id) DO UPDATE SET \
              nickname=excluded.nickname, \
              title=excluded.title, description=excluded.description, \
              tags=excluded.tags, uploaded_at=excluded.uploaded_at, \
              message_count=excluded.message_count, task_count=excluded.task_count, \
              event_count=excluded.event_count, duration_seconds=excluded.duration_seconds, \
              total_input_tokens=excluded.total_input_tokens, \
              total_output_tokens=excluded.total_output_tokens \
              WHERE sync_status = 'remote_only'",
            params![
                &summary.id,
                &summary.user_id,
                &summary.nickname,
                &summary.team_id,
                &summary.tool,
                &summary.agent_provider,
                &summary.agent_model,
                &summary.title,
                &summary.description,
                &summary.tags,
                &summary.created_at,
                &summary.uploaded_at,
                summary.message_count,
                summary.task_count,
                summary.event_count,
                summary.duration_seconds,
                summary.total_input_tokens,
                summary.total_output_tokens,
            ],
        )?;
        Ok(())
    }

    // ── List sessions ──────────────────────────────────────────────────

    pub fn list_sessions(&self, filter: &LocalSessionFilter) -> Result<Vec<LocalSessionRow>> {
        let mut where_clauses = vec!["1=1".to_string()];
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1u32;

        if let Some(ref team_id) = filter.team_id {
            where_clauses.push(format!("team_id = ?{idx}"));
            param_values.push(Box::new(team_id.clone()));
            idx += 1;
        }

        if let Some(ref sync_status) = filter.sync_status {
            where_clauses.push(format!("sync_status = ?{idx}"));
            param_values.push(Box::new(sync_status.clone()));
            idx += 1;
        }

        if let Some(ref repo) = filter.git_repo_name {
            where_clauses.push(format!("git_repo_name = ?{idx}"));
            param_values.push(Box::new(repo.clone()));
            idx += 1;
        }

        if let Some(ref tool) = filter.tool {
            where_clauses.push(format!("tool = ?{idx}"));
            param_values.push(Box::new(tool.clone()));
            idx += 1;
        }

        if let Some(ref search) = filter.search {
            let like = format!("%{search}%");
            where_clauses.push(format!(
                "(title LIKE ?{i1} OR description LIKE ?{i2} OR tags LIKE ?{i3})",
                i1 = idx,
                i2 = idx + 1,
                i3 = idx + 2,
            ));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like.clone()));
            param_values.push(Box::new(like));
            // idx += 3; // not needed after last use
        }

        let where_str = where_clauses.join(" AND ");
        let sql = format!(
            "SELECT id, source_path, sync_status, last_synced_at, \
                    user_id, nickname, team_id, tool, agent_provider, agent_model, \
                    title, description, tags, created_at, uploaded_at, \
                    message_count, task_count, event_count, duration_seconds, \
                    total_input_tokens, total_output_tokens, \
                    git_remote, git_branch, git_commit, git_repo_name, \
                    pr_number, pr_url, working_directory \
             FROM local_sessions WHERE {where_str} \
             ORDER BY created_at DESC"
        );

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), row_to_local_session)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ── Sync cursor ────────────────────────────────────────────────────

    pub fn get_sync_cursor(&self, team_id: &str) -> Result<Option<String>> {
        let cursor = self
            .conn()
            .query_row(
                "SELECT cursor FROM sync_cursors WHERE team_id = ?1",
                params![team_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(cursor)
    }

    pub fn set_sync_cursor(&self, team_id: &str, cursor: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO sync_cursors (team_id, cursor, updated_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(team_id) DO UPDATE SET cursor=excluded.cursor, updated_at=datetime('now')",
            params![team_id, cursor],
        )?;
        Ok(())
    }

    // ── Upload tracking ────────────────────────────────────────────────

    /// Get sessions that are local_only and need to be uploaded.
    pub fn pending_uploads(&self, team_id: &str) -> Result<Vec<LocalSessionRow>> {
        let sql = "SELECT id, source_path, sync_status, last_synced_at, \
                          user_id, nickname, team_id, tool, agent_provider, agent_model, \
                          title, description, tags, created_at, uploaded_at, \
                          message_count, task_count, event_count, duration_seconds, \
                          total_input_tokens, total_output_tokens, \
                          git_remote, git_branch, git_commit, git_repo_name, \
                          pr_number, pr_url, working_directory \
                   FROM local_sessions WHERE sync_status = 'local_only' AND team_id = ?1 \
                   ORDER BY created_at ASC";
        let conn = self.conn();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![team_id], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn mark_synced(&self, session_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE local_sessions SET sync_status = 'synced', last_synced_at = datetime('now') \
             WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Check if a session was already uploaded (synced or remote_only) since the given modification time.
    pub fn was_uploaded_after(
        &self,
        source_path: &str,
        modified: &chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        let result: Option<String> = self
            .conn()
            .query_row(
                "SELECT last_synced_at FROM local_sessions \
                 WHERE source_path = ?1 AND sync_status = 'synced' AND last_synced_at IS NOT NULL",
                params![source_path],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(synced_at) = result {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&synced_at) {
                return Ok(dt >= *modified);
            }
        }
        Ok(false)
    }

    // ── Body cache ─────────────────────────────────────────────────────

    pub fn cache_body(&self, session_id: &str, body: &[u8]) -> Result<()> {
        self.conn().execute(
            "INSERT INTO body_cache (session_id, body, cached_at) \
             VALUES (?1, ?2, datetime('now')) \
             ON CONFLICT(session_id) DO UPDATE SET body=excluded.body, cached_at=datetime('now')",
            params![session_id, body],
        )?;
        Ok(())
    }

    pub fn get_cached_body(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
        let body = self
            .conn()
            .query_row(
                "SELECT body FROM body_cache WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body)
    }

    // ── Migration helper ───────────────────────────────────────────────

    /// Migrate entries from the old state.json UploadState into the local DB.
    /// Marks them as `synced` with no metadata (we only know the file path was uploaded).
    pub fn migrate_from_state_json(
        &self,
        uploaded: &std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
    ) -> Result<usize> {
        let mut count = 0;
        for (path, uploaded_at) in uploaded {
            let exists: bool = self
                .conn()
                .query_row(
                    "SELECT COUNT(*) > 0 FROM local_sessions WHERE source_path = ?1",
                    params![path],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            if exists {
                self.conn().execute(
                    "UPDATE local_sessions SET sync_status = 'synced', last_synced_at = ?1 \
                     WHERE source_path = ?2 AND sync_status = 'local_only'",
                    params![uploaded_at.to_rfc3339(), path],
                )?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Get a list of distinct git repo names present in the DB.
    pub fn list_repos(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT git_repo_name FROM local_sessions \
             WHERE git_repo_name IS NOT NULL ORDER BY git_repo_name ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

fn row_to_local_session(row: &rusqlite::Row) -> rusqlite::Result<LocalSessionRow> {
    Ok(LocalSessionRow {
        id: row.get(0)?,
        source_path: row.get(1)?,
        sync_status: row.get(2)?,
        last_synced_at: row.get(3)?,
        user_id: row.get(4)?,
        nickname: row.get(5)?,
        team_id: row.get(6)?,
        tool: row.get(7)?,
        agent_provider: row.get(8)?,
        agent_model: row.get(9)?,
        title: row.get(10)?,
        description: row.get(11)?,
        tags: row.get(12)?,
        created_at: row.get(13)?,
        uploaded_at: row.get(14)?,
        message_count: row.get(15)?,
        task_count: row.get(16)?,
        event_count: row.get(17)?,
        duration_seconds: row.get(18)?,
        total_input_tokens: row.get(19)?,
        total_output_tokens: row.get(20)?,
        git_remote: row.get(21)?,
        git_branch: row.get(22)?,
        git_commit: row.get(23)?,
        git_repo_name: row.get(24)?,
        pr_number: row.get(25)?,
        pr_url: row.get(26)?,
        working_directory: row.get(27)?,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> LocalDb {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("test.db");
        LocalDb::open_path(&path).unwrap()
    }

    #[test]
    fn test_open_and_schema() {
        let _db = test_db();
    }

    #[test]
    fn test_sync_cursor() {
        let db = test_db();
        assert_eq!(db.get_sync_cursor("team1").unwrap(), None);
        db.set_sync_cursor("team1", "2024-01-01T00:00:00Z")
            .unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-01-01T00:00:00Z".to_string())
        );
        // Update
        db.set_sync_cursor("team1", "2024-06-01T00:00:00Z")
            .unwrap();
        assert_eq!(
            db.get_sync_cursor("team1").unwrap(),
            Some("2024-06-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_body_cache() {
        let db = test_db();
        assert_eq!(db.get_cached_body("s1").unwrap(), None);
        db.cache_body("s1", b"hello world").unwrap();
        assert_eq!(db.get_cached_body("s1").unwrap(), Some(b"hello world".to_vec()));
    }

    #[test]
    fn test_upsert_remote_session() {
        let db = test_db();
        let summary = SessionSummary {
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
        };
        db.upsert_remote_session(&summary).unwrap();

        let sessions = db.list_sessions(&LocalSessionFilter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "remote-1");
        assert_eq!(sessions[0].sync_status, "remote_only");
        assert_eq!(sessions[0].nickname, Some("alice".to_string()));
    }

    #[test]
    fn test_list_filter_by_repo() {
        let db = test_db();
        // Insert a remote session with team_id
        let summary1 = SessionSummary {
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
}
