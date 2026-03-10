use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::connection::LocalDb;
use crate::session_store::{LOCAL_SESSION_COLUMNS, LocalSessionRow, row_to_local_session};

impl LocalDb {
    /// Fetch the source path used when the session was last parsed/loaded.
    pub fn get_session_source_path(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn();
        let result = conn
            .query_row(
                "SELECT source_path FROM session_sync WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;

        Ok(result)
    }

    /// List every session id with a non-empty source path from session_sync.
    pub fn list_session_source_paths(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, source_path \
             FROM session_sync \
             WHERE source_path IS NOT NULL AND TRIM(source_path) != ''",
        )?;
        let rows = stmt.query_map([], |row| {
            let session_id: String = row.get(0)?;
            let source_path: String = row.get(1)?;
            Ok((session_id, source_path))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

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

    /// Get sessions that are local_only and need to be uploaded.
    pub fn pending_uploads(&self, team_id: &str) -> Result<Vec<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             FROM sessions s \
             INNER JOIN session_sync ss ON ss.session_id = s.id \
             LEFT JOIN users u ON u.id = s.user_id \
             WHERE ss.sync_status = 'local_only' AND s.team_id = ?1 AND COALESCE(s.is_auxiliary, 0) = 0 \
             ORDER BY s.created_at ASC"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![team_id], row_to_local_session)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn mark_synced(&self, session_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE session_sync SET sync_status = 'synced', last_synced_at = datetime('now') \
             WHERE session_id = ?1",
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
                "SELECT last_synced_at FROM session_sync \
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

    /// Update only sync metadata path for an existing session.
    pub fn set_session_sync_path(&self, session_id: &str, source_path: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO session_sync (session_id, source_path) \
             VALUES (?1, ?2) \
             ON CONFLICT(session_id) DO UPDATE SET source_path = excluded.source_path",
            params![session_id, source_path],
        )?;
        Ok(())
    }
}
