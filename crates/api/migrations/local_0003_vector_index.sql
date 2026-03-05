-- Local semantic vector index cache (Desktop)
CREATE TABLE IF NOT EXISTS vector_chunks (
    id           TEXT PRIMARY KEY,
    session_id   TEXT NOT NULL,
    chunk_index  INTEGER NOT NULL,
    start_line   INTEGER NOT NULL,
    end_line     INTEGER NOT NULL,
    line_count   INTEGER NOT NULL,
    content      TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_vector_chunks_session_chunk
    ON vector_chunks(session_id, chunk_index);
CREATE INDEX IF NOT EXISTS idx_vector_chunks_session
    ON vector_chunks(session_id);

CREATE TABLE IF NOT EXISTS vector_embeddings (
    chunk_id        TEXT PRIMARY KEY,
    model           TEXT NOT NULL,
    embedding_dim   INTEGER NOT NULL,
    embedding_json  TEXT NOT NULL,
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_vector_embeddings_model
    ON vector_embeddings(model);

CREATE TABLE IF NOT EXISTS vector_index_sessions (
    session_id      TEXT PRIMARY KEY,
    source_hash     TEXT NOT NULL,
    chunk_count     INTEGER NOT NULL DEFAULT 0,
    last_indexed_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS vector_index_jobs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    status             TEXT NOT NULL,
    processed_sessions INTEGER NOT NULL DEFAULT 0,
    total_sessions     INTEGER NOT NULL DEFAULT 0,
    message            TEXT,
    started_at         TEXT,
    finished_at        TEXT,
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_vector_index_jobs_status
    ON vector_index_jobs(status, updated_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS vector_chunks_fts USING fts5(
    chunk_id UNINDEXED,
    session_id UNINDEXED,
    content
);
