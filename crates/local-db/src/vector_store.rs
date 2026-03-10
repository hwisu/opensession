use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use crate::connection::LocalDb;

/// Vector chunk payload persisted per session.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorChunkUpsert {
    pub chunk_id: String,
    pub session_id: String,
    pub chunk_index: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub line_count: u32,
    pub content: String,
    pub content_hash: String,
    pub embedding: Vec<f32>,
}

/// Candidate row used for local semantic vector ranking.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorChunkCandidateRow {
    pub chunk_id: String,
    pub session_id: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub embedding: Vec<f32>,
}

pub(crate) fn build_fts_query(raw: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for token in raw.split_whitespace() {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        let escaped = trimmed.replace('"', "\"\"");
        parts.push(format!("\"{escaped}\""));
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" OR "))
}

impl LocalDb {
    pub fn vector_index_source_hash(&self, session_id: &str) -> Result<Option<String>> {
        let hash = self
            .conn()
            .query_row(
                "SELECT source_hash FROM vector_index_sessions WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    pub fn clear_vector_index(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM vector_embeddings", [])?;
        conn.execute("DELETE FROM vector_chunks_fts", [])?;
        conn.execute("DELETE FROM vector_chunks", [])?;
        conn.execute("DELETE FROM vector_index_sessions", [])?;
        Ok(())
    }

    pub fn replace_session_vector_chunks(
        &self,
        session_id: &str,
        source_hash: &str,
        model: &str,
        chunks: &[VectorChunkUpsert],
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        tx.execute(
            "DELETE FROM vector_embeddings \
             WHERE chunk_id IN (SELECT id FROM vector_chunks WHERE session_id = ?1)",
            params![session_id],
        )?;
        tx.execute(
            "DELETE FROM vector_chunks_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        tx.execute(
            "DELETE FROM vector_chunks WHERE session_id = ?1",
            params![session_id],
        )?;

        for chunk in chunks {
            let embedding_json = serde_json::to_string(&chunk.embedding)
                .context("serialize vector embedding for local cache")?;
            tx.execute(
                "INSERT INTO vector_chunks \
                 (id, session_id, chunk_index, start_line, end_line, line_count, content, content_hash, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'), datetime('now'))",
                params![
                    &chunk.chunk_id,
                    &chunk.session_id,
                    chunk.chunk_index as i64,
                    chunk.start_line as i64,
                    chunk.end_line as i64,
                    chunk.line_count as i64,
                    &chunk.content,
                    &chunk.content_hash,
                ],
            )?;
            tx.execute(
                "INSERT INTO vector_embeddings \
                 (chunk_id, model, embedding_dim, embedding_json, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
                params![
                    &chunk.chunk_id,
                    model,
                    chunk.embedding.len() as i64,
                    &embedding_json
                ],
            )?;
            tx.execute(
                "INSERT INTO vector_chunks_fts (chunk_id, session_id, content) VALUES (?1, ?2, ?3)",
                params![&chunk.chunk_id, &chunk.session_id, &chunk.content],
            )?;
        }

        tx.execute(
            "INSERT INTO vector_index_sessions \
             (session_id, source_hash, chunk_count, last_indexed_at, updated_at) \
             VALUES (?1, ?2, ?3, datetime('now'), datetime('now')) \
             ON CONFLICT(session_id) DO UPDATE SET \
             source_hash=excluded.source_hash, \
             chunk_count=excluded.chunk_count, \
             last_indexed_at=datetime('now'), \
             updated_at=datetime('now')",
            params![session_id, source_hash, chunks.len() as i64],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn list_vector_chunk_candidates(
        &self,
        query: &str,
        model: &str,
        limit: u32,
    ) -> Result<Vec<VectorChunkCandidateRow>> {
        let Some(fts_query) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.start_line, c.end_line, c.content, e.embedding_json \
             FROM vector_chunks_fts f \
             INNER JOIN vector_chunks c ON c.id = f.chunk_id \
             INNER JOIN vector_embeddings e ON e.chunk_id = c.id \
             WHERE f.content MATCH ?1 AND e.model = ?2 \
             ORDER BY bm25(vector_chunks_fts) ASC, c.updated_at DESC \
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![fts_query, model, limit as i64], |row| {
            let embedding_json: String = row.get(5)?;
            let embedding =
                serde_json::from_str::<Vec<f32>>(&embedding_json).unwrap_or_else(|_| Vec::new());
            Ok(VectorChunkCandidateRow {
                chunk_id: row.get(0)?,
                session_id: row.get(1)?,
                start_line: row.get::<_, i64>(2)?.max(0) as u32,
                end_line: row.get::<_, i64>(3)?.max(0) as u32,
                content: row.get(4)?,
                embedding,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_recent_vector_chunks_for_model(
        &self,
        model: &str,
        limit: u32,
    ) -> Result<Vec<VectorChunkCandidateRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.start_line, c.end_line, c.content, e.embedding_json \
             FROM vector_chunks c \
             INNER JOIN vector_embeddings e ON e.chunk_id = c.id \
             WHERE e.model = ?1 \
             ORDER BY c.updated_at DESC \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model, limit as i64], |row| {
            let embedding_json: String = row.get(5)?;
            let embedding =
                serde_json::from_str::<Vec<f32>>(&embedding_json).unwrap_or_else(|_| Vec::new());
            Ok(VectorChunkCandidateRow {
                chunk_id: row.get(0)?,
                session_id: row.get(1)?,
                start_line: row.get::<_, i64>(2)?.max(0) as u32,
                end_line: row.get::<_, i64>(3)?.max(0) as u32,
                content: row.get(4)?,
                embedding,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}
