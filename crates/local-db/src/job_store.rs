use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::connection::LocalDb;

/// Vector indexing progress/status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexJobRow {
    pub status: String,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Summary batch generation progress/status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryBatchJobRow {
    pub status: String,
    pub processed_sessions: u32,
    pub total_sessions: u32,
    pub failed_sessions: u32,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Lifecycle cleanup progress/status snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleCleanupJobRow {
    pub status: String,
    pub deleted_sessions: u32,
    pub deleted_summaries: u32,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl LocalDb {
    pub fn set_vector_index_job(&self, payload: &VectorIndexJobRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO vector_index_jobs \
             (id, status, processed_sessions, total_sessions, message, started_at, finished_at, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             status=excluded.status, \
             processed_sessions=excluded.processed_sessions, \
             total_sessions=excluded.total_sessions, \
             message=excluded.message, \
             started_at=excluded.started_at, \
             finished_at=excluded.finished_at, \
             updated_at=datetime('now')",
            params![
                payload.status,
                payload.processed_sessions as i64,
                payload.total_sessions as i64,
                payload.message,
                payload.started_at,
                payload.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_vector_index_job(&self) -> Result<Option<VectorIndexJobRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT status, processed_sessions, total_sessions, message, started_at, finished_at \
                 FROM vector_index_jobs WHERE id = 1 LIMIT 1",
                [],
                |row| {
                    Ok(VectorIndexJobRow {
                        status: row.get(0)?,
                        processed_sessions: row.get::<_, i64>(1)?.max(0) as u32,
                        total_sessions: row.get::<_, i64>(2)?.max(0) as u32,
                        message: row.get(3)?,
                        started_at: row.get(4)?,
                        finished_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn set_summary_batch_job(&self, payload: &SummaryBatchJobRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO summary_batch_jobs \
             (id, status, processed_sessions, total_sessions, failed_sessions, message, started_at, finished_at, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             status=excluded.status, \
             processed_sessions=excluded.processed_sessions, \
             total_sessions=excluded.total_sessions, \
             failed_sessions=excluded.failed_sessions, \
             message=excluded.message, \
             started_at=excluded.started_at, \
             finished_at=excluded.finished_at, \
             updated_at=datetime('now')",
            params![
                payload.status,
                payload.processed_sessions as i64,
                payload.total_sessions as i64,
                payload.failed_sessions as i64,
                payload.message,
                payload.started_at,
                payload.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_summary_batch_job(&self) -> Result<Option<SummaryBatchJobRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT status, processed_sessions, total_sessions, failed_sessions, message, started_at, finished_at \
                 FROM summary_batch_jobs WHERE id = 1 LIMIT 1",
                [],
                |row| {
                    Ok(SummaryBatchJobRow {
                        status: row.get(0)?,
                        processed_sessions: row.get::<_, i64>(1)?.max(0) as u32,
                        total_sessions: row.get::<_, i64>(2)?.max(0) as u32,
                        failed_sessions: row.get::<_, i64>(3)?.max(0) as u32,
                        message: row.get(4)?,
                        started_at: row.get(5)?,
                        finished_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn set_lifecycle_cleanup_job(&self, payload: &LifecycleCleanupJobRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO lifecycle_cleanup_jobs \
             (id, status, deleted_sessions, deleted_summaries, message, started_at, finished_at, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, datetime('now')) \
             ON CONFLICT(id) DO UPDATE SET \
             status=excluded.status, \
             deleted_sessions=excluded.deleted_sessions, \
             deleted_summaries=excluded.deleted_summaries, \
             message=excluded.message, \
             started_at=excluded.started_at, \
             finished_at=excluded.finished_at, \
             updated_at=datetime('now')",
            params![
                payload.status,
                payload.deleted_sessions as i64,
                payload.deleted_summaries as i64,
                payload.message,
                payload.started_at,
                payload.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_lifecycle_cleanup_job(&self) -> Result<Option<LifecycleCleanupJobRow>> {
        let row = self
            .conn()
            .query_row(
                "SELECT status, deleted_sessions, deleted_summaries, message, started_at, finished_at \
                 FROM lifecycle_cleanup_jobs WHERE id = 1 LIMIT 1",
                [],
                |row| {
                    Ok(LifecycleCleanupJobRow {
                        status: row.get(0)?,
                        deleted_sessions: row.get::<_, i64>(1)?.max(0) as u32,
                        deleted_summaries: row.get::<_, i64>(2)?.max(0) as u32,
                        message: row.get(3)?,
                        started_at: row.get(4)?,
                        finished_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}
