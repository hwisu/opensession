-- OpenSession schema (consolidated)
-- This is the canonical schema for both server (SQLite) and worker (D1).

-- Users
CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    nickname      TEXT NOT NULL UNIQUE,
    api_key       TEXT NOT NULL UNIQUE,
    email         TEXT,
    password_hash TEXT,
    password_salt TEXT,
    avatar_url    TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email) WHERE email IS NOT NULL;

-- Teams
CREATE TABLE IF NOT EXISTS teams (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    is_public   BOOLEAN NOT NULL DEFAULT 0,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Team members
CREATE TABLE IF NOT EXISTS team_members (
    team_id   TEXT NOT NULL REFERENCES teams(id),
    user_id   TEXT NOT NULL REFERENCES users(id),
    role      TEXT NOT NULL DEFAULT 'member',
    joined_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (team_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_team_members_user_id ON team_members(user_id);

-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    id                  TEXT PRIMARY KEY,
    user_id             TEXT REFERENCES users(id),
    team_id             TEXT NOT NULL REFERENCES teams(id),
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
    has_errors          BOOLEAN DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_sessions_team_id ON sessions(team_id);
CREATE INDEX IF NOT EXISTS idx_sessions_uploaded_at ON sessions(uploaded_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_tool ON sessions(tool);

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

-- Team invitations
CREATE TABLE IF NOT EXISTS team_invitations (
    id                      TEXT PRIMARY KEY,
    team_id                 TEXT NOT NULL REFERENCES teams(id),
    email                   TEXT,
    oauth_provider          TEXT,
    oauth_provider_username TEXT,
    invited_by              TEXT NOT NULL REFERENCES users(id),
    role                    TEXT NOT NULL DEFAULT 'member',
    status                  TEXT NOT NULL DEFAULT 'pending',
    created_at              TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at              TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_invitations_email ON team_invitations(email) WHERE email IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_invitations_oauth ON team_invitations(oauth_provider, oauth_provider_username) WHERE oauth_provider IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_invitations_team ON team_invitations(team_id);
