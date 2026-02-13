-- Local-only schema (TUI + Daemon)
-- Runs AFTER the shared 0001_schema.sql on local SQLite databases.

-- Local sessions: unified store for local + synced sessions
CREATE TABLE IF NOT EXISTS local_sessions (
    id                  TEXT PRIMARY KEY,
    source_path         TEXT,
    sync_status         TEXT NOT NULL DEFAULT 'local_only',
    last_synced_at      TEXT,
    user_id             TEXT,
    nickname            TEXT,
    team_id             TEXT,
    tool                TEXT NOT NULL,
    agent_provider      TEXT,
    agent_model         TEXT,
    title               TEXT,
    description         TEXT,
    tags                TEXT,
    created_at          TEXT NOT NULL,
    uploaded_at         TEXT,
    message_count       INTEGER DEFAULT 0,
    task_count          INTEGER DEFAULT 0,
    event_count         INTEGER DEFAULT 0,
    duration_seconds    INTEGER DEFAULT 0,
    total_input_tokens  INTEGER DEFAULT 0,
    total_output_tokens INTEGER DEFAULT 0,
    git_remote          TEXT,
    git_branch          TEXT,
    git_commit          TEXT,
    git_repo_name       TEXT,
    pr_number           INTEGER,
    pr_url              TEXT,
    working_directory   TEXT,
    files_modified      TEXT,
    files_read          TEXT,
    has_errors          BOOLEAN DEFAULT 0,
    user_message_count  INTEGER DEFAULT 0
);

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
