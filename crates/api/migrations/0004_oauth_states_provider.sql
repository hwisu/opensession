-- Ensure oauth_states always includes provider.
-- Legacy deployments may have oauth_states without provider due old 0001 schema.
-- This table only stores short-lived CSRF state tokens, so rebuild is safe.

DROP TABLE IF EXISTS oauth_states;

CREATE TABLE IF NOT EXISTS oauth_states (
    state      TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    user_id    TEXT
);
