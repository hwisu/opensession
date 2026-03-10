use anyhow::Result;

use crate::connection::LocalDb;
use crate::session_store::{
    FROM_CLAUSE, LOCAL_SESSION_COLUMNS, LocalSessionRow, row_to_local_session,
};

impl LocalDb {
    /// Find the most recently active session for a given repo path.
    /// "Active" means the session's working_directory matches the repo path
    /// and was created within the last `since_minutes` minutes.
    pub fn find_active_session_for_repo(
        &self,
        repo_path: &str,
        since_minutes: u32,
    ) -> Result<Option<LocalSessionRow>> {
        let sql = format!(
            "SELECT {LOCAL_SESSION_COLUMNS} \
             {FROM_CLAUSE} \
             WHERE s.working_directory LIKE ?1 \
             AND COALESCE(s.is_auxiliary, 0) = 0 \
             AND s.created_at >= datetime('now', ?2) \
             ORDER BY s.created_at DESC LIMIT 1"
        );
        let since = format!("-{since_minutes} minutes");
        let like = format!("{repo_path}%");
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let row = stmt
            .query_map(rusqlite::params![like, since], row_to_local_session)?
            .next()
            .transpose()?;
        Ok(row)
    }

    /// Get a list of distinct git repo names present in the DB.
    pub fn list_repos(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT git_repo_name FROM sessions \
             WHERE git_repo_name IS NOT NULL AND COALESCE(is_auxiliary, 0) = 0 \
             ORDER BY git_repo_name ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get a list of distinct, non-empty working directories present in the DB.
    pub fn list_working_directories(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT working_directory FROM sessions \
             WHERE working_directory IS NOT NULL AND TRIM(working_directory) <> '' \
             AND COALESCE(is_auxiliary, 0) = 0 \
             ORDER BY working_directory ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}
