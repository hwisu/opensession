-- Users (MVP: nickname-based, API key auth)
CREATE TABLE users (
    id          TEXT PRIMARY KEY,
    nickname    TEXT NOT NULL UNIQUE,
    api_key     TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Groups
CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    is_public   BOOLEAN NOT NULL DEFAULT 0,
    owner_id    TEXT NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Group members
CREATE TABLE group_members (
    group_id    TEXT NOT NULL REFERENCES groups(id),
    user_id     TEXT NOT NULL REFERENCES users(id),
    role        TEXT NOT NULL DEFAULT 'member',
    joined_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (group_id, user_id)
);

-- Invites
CREATE TABLE invites (
    id          TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups(id),
    code        TEXT NOT NULL UNIQUE,
    created_by  TEXT NOT NULL REFERENCES users(id),
    max_uses    INTEGER,
    used_count  INTEGER NOT NULL DEFAULT 0,
    expires_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Sessions
CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,
    user_id             TEXT REFERENCES users(id),
    tool                TEXT NOT NULL,
    agent_provider      TEXT,
    agent_model         TEXT,
    title               TEXT,
    description         TEXT,
    tags                TEXT,
    visibility          TEXT NOT NULL DEFAULT 'public',
    created_at          TEXT NOT NULL,
    uploaded_at         TEXT NOT NULL DEFAULT (datetime('now')),
    message_count       INTEGER DEFAULT 0,
    task_count          INTEGER DEFAULT 0,
    event_count         INTEGER DEFAULT 0,
    duration_seconds    INTEGER DEFAULT 0,
    body_storage_key    TEXT NOT NULL
);

-- Session-group link
CREATE TABLE session_groups (
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    group_id    TEXT NOT NULL REFERENCES groups(id),
    PRIMARY KEY (session_id, group_id)
);

-- FTS5 search
CREATE VIRTUAL TABLE sessions_fts USING fts5(
    title, description, tags, content='sessions', content_rowid='rowid'
);
