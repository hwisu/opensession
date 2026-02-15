-- Local-only schema (TUI + Daemon)
-- Runs AFTER the shared 0001_schema.sql on local SQLite databases.

-- Sync tracking
CREATE TABLE IF NOT EXISTS session_sync (
    session_id     TEXT PRIMARY KEY,
    source_path    TEXT,
    sync_status    TEXT NOT NULL DEFAULT 'local_only',
    last_synced_at TEXT
);

-- Sync cursors per team
CREATE TABLE IF NOT EXISTS sync_cursors (
    team_id    TEXT NOT NULL,
    cursor     TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (team_id)
);

-- Body cache for full session bodies
CREATE TABLE IF NOT EXISTS body_cache (
    session_id TEXT PRIMARY KEY,
    body       BLOB,
    cached_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Commit <-> session linking
CREATE TABLE IF NOT EXISTS commit_session_links (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    commit_hash TEXT NOT NULL,
    session_id  TEXT NOT NULL,
    repo_path   TEXT,
    branch      TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(commit_hash, session_id)
);
CREATE INDEX IF NOT EXISTS idx_commit_links_hash ON commit_session_links(commit_hash);
CREATE INDEX IF NOT EXISTS idx_commit_links_session ON commit_session_links(session_id);
