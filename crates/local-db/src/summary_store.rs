use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::connection::LocalDb;

/// Session-level semantic summary row persisted in local SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSemanticSummaryRow {
    pub session_id: String,
    pub summary_json: String,
    pub generated_at: String,
    pub provider: String,
    pub model: Option<String>,
    pub source_kind: String,
    pub generation_kind: String,
    pub prompt_fingerprint: Option<String>,
    pub source_details_json: Option<String>,
    pub diff_tree_json: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

/// Upsert payload for session-level semantic summaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSemanticSummaryUpsert<'a> {
    pub session_id: &'a str,
    pub summary_json: &'a str,
    pub generated_at: &'a str,
    pub provider: &'a str,
    pub model: Option<&'a str>,
    pub source_kind: &'a str,
    pub generation_kind: &'a str,
    pub prompt_fingerprint: Option<&'a str>,
    pub source_details_json: Option<&'a str>,
    pub diff_tree_json: Option<&'a str>,
    pub error: Option<&'a str>,
}

impl LocalDb {
    pub fn upsert_session_semantic_summary(
        &self,
        payload: &SessionSemanticSummaryUpsert<'_>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO session_semantic_summaries (\
                session_id, summary_json, generated_at, provider, model, \
                source_kind, generation_kind, prompt_fingerprint, source_details_json, \
                diff_tree_json, error, updated_at\
             ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, \
                ?6, ?7, ?8, ?9, \
                ?10, ?11, datetime('now')\
             ) \
             ON CONFLICT(session_id) DO UPDATE SET \
                summary_json=excluded.summary_json, \
                generated_at=excluded.generated_at, \
                provider=excluded.provider, \
                model=excluded.model, \
                source_kind=excluded.source_kind, \
                generation_kind=excluded.generation_kind, \
                prompt_fingerprint=excluded.prompt_fingerprint, \
                source_details_json=excluded.source_details_json, \
                diff_tree_json=excluded.diff_tree_json, \
                error=excluded.error, \
                updated_at=datetime('now')",
            params![
                payload.session_id,
                payload.summary_json,
                payload.generated_at,
                payload.provider,
                payload.model,
                payload.source_kind,
                payload.generation_kind,
                payload.prompt_fingerprint,
                payload.source_details_json,
                payload.diff_tree_json,
                payload.error,
            ],
        )?;
        Ok(())
    }

    pub fn list_expired_session_ids(&self, keep_days: u32) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM sessions \
             WHERE julianday(created_at) <= julianday('now') - ?1 \
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![keep_days as i64], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// List all known session ids for migration or maintenance workflows.
    pub fn list_all_session_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM sessions ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// List all session ids that currently have cached semantic summaries.
    pub fn list_session_semantic_summary_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT session_id FROM session_semantic_summaries ORDER BY session_id ASC")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_session_semantic_summary(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionSemanticSummaryRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT session_id, summary_json, generated_at, provider, model, \
                        source_kind, generation_kind, prompt_fingerprint, source_details_json, \
                        diff_tree_json, error, updated_at \
                 FROM session_semantic_summaries WHERE session_id = ?1 LIMIT 1",
                params![session_id],
                |row| {
                    Ok(SessionSemanticSummaryRow {
                        session_id: row.get(0)?,
                        summary_json: row.get(1)?,
                        generated_at: row.get(2)?,
                        provider: row.get(3)?,
                        model: row.get(4)?,
                        source_kind: row.get(5)?,
                        generation_kind: row.get(6)?,
                        prompt_fingerprint: row.get(7)?,
                        source_details_json: row.get(8)?,
                        diff_tree_json: row.get(9)?,
                        error: row.get(10)?,
                        updated_at: row.get(11)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn delete_expired_session_summaries(&self, keep_days: u32) -> Result<u32> {
        let deleted = self.conn().execute(
            "DELETE FROM session_semantic_summaries \
             WHERE julianday(generated_at) <= julianday('now') - ?1",
            params![keep_days as i64],
        )?;
        Ok(deleted as u32)
    }
}
