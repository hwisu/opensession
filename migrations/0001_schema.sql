-- OpenSession schema (single bootstrap, no compatibility migrations)
-- Canonical schema for server (SQLite) and worker (D1).

-- Users
CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    nickname      TEXT NOT NULL UNIQUE,
    email         TEXT,
    password_hash TEXT,
    password_salt TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email) WHERE email IS NOT NULL;

-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    id                  TEXT PRIMARY KEY,
    user_id             TEXT,
    team_id             TEXT NOT NULL,
    tool                TEXT NOT NULL,
    agent_provider      TEXT,
    agent_model         TEXT,
    title               TEXT,
    description         TEXT,
    tags                TEXT,
    created_at          TEXT NOT NULL,
    uploaded_at         TEXT NOT NULL DEFAULT (datetime('now')),
    message_count       INTEGER DEFAULT 0,
    user_message_count  INTEGER DEFAULT 0,
    task_count          INTEGER DEFAULT 0,
    event_count         INTEGER DEFAULT 0,
    duration_seconds    INTEGER DEFAULT 0,
    total_input_tokens  INTEGER NOT NULL DEFAULT 0,
    total_output_tokens INTEGER NOT NULL DEFAULT 0,
    body_storage_key    TEXT NOT NULL,
    body_url            TEXT,
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
    max_active_agents   INTEGER NOT NULL DEFAULT 1,
    session_score       INTEGER NOT NULL DEFAULT 0,
    score_plugin        TEXT NOT NULL DEFAULT 'heuristic_v1'
);
CREATE INDEX IF NOT EXISTS idx_sessions_uploaded_at ON sessions(uploaded_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_tool ON sessions(tool);
CREATE INDEX IF NOT EXISTS idx_sessions_visible_created_at
ON sessions(created_at DESC)
WHERE event_count > 0 OR message_count > 0;
CREATE INDEX IF NOT EXISTS idx_sessions_visible_tool_created_at
ON sessions(tool, created_at DESC)
WHERE event_count > 0 OR message_count > 0;
CREATE INDEX IF NOT EXISTS idx_sessions_visible_popular
ON sessions(message_count DESC, created_at DESC)
WHERE event_count > 0 OR message_count > 0;
CREATE INDEX IF NOT EXISTS idx_sessions_visible_longest
ON sessions(duration_seconds DESC, created_at DESC)
WHERE event_count > 0 OR message_count > 0;
CREATE INDEX IF NOT EXISTS idx_sessions_session_score ON sessions(session_score DESC);

-- Session links (handoff chains, etc.)
CREATE TABLE IF NOT EXISTS session_links (
    session_id        TEXT NOT NULL,
    linked_session_id TEXT NOT NULL,
    link_type         TEXT NOT NULL DEFAULT 'handoff',
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (session_id, linked_session_id)
);
CREATE INDEX IF NOT EXISTS idx_session_links_linked ON session_links(linked_session_id);

-- OAuth identities
CREATE TABLE IF NOT EXISTS oauth_identities (
    user_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider          TEXT NOT NULL,
    provider_user_id  TEXT NOT NULL,
    provider_username TEXT,
    avatar_url        TEXT,
    instance_url      TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (provider, provider_user_id),
    UNIQUE (user_id, provider)
);

-- OAuth state tokens (CSRF)
CREATE TABLE IF NOT EXISTS oauth_states (
    state      TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    user_id    TEXT
);

-- Refresh tokens
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- API key issuance table (hash-only persistence).
CREATE TABLE IF NOT EXISTS api_keys (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_hash     TEXT NOT NULL UNIQUE,
    key_prefix   TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active',
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    grace_until  TEXT,
    revoked_at   TEXT,
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_api_keys_user_status ON api_keys(user_id, status);
CREATE INDEX IF NOT EXISTS idx_api_keys_grace_until ON api_keys(grace_until);
