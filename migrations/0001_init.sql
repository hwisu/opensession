-- Users: nickname + API key auth, first user becomes admin
CREATE TABLE users (
    id          TEXT PRIMARY KEY,
    nickname    TEXT NOT NULL UNIQUE,
    api_key     TEXT NOT NULL UNIQUE,
    is_admin    BOOLEAN NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Teams
CREATE TABLE teams (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Team members
CREATE TABLE team_members (
    team_id     TEXT NOT NULL REFERENCES teams(id),
    user_id     TEXT NOT NULL REFERENCES users(id),
    role        TEXT NOT NULL DEFAULT 'member',
    joined_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (team_id, user_id)
);

-- Sessions (scoped to a team)
CREATE TABLE sessions (
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
    task_count          INTEGER DEFAULT 0,
    event_count         INTEGER DEFAULT 0,
    duration_seconds    INTEGER DEFAULT 0,
    body_storage_key    TEXT NOT NULL
);

-- FTS5 search
CREATE VIRTUAL TABLE sessions_fts USING fts5(
    title, description, tags, content='sessions', content_rowid='rowid'
);
