-- Add authentication infrastructure: email/password, OAuth, refresh tokens.

-- Extend users table for email/password auth
ALTER TABLE users ADD COLUMN email TEXT;
ALTER TABLE users ADD COLUMN password_hash TEXT;
ALTER TABLE users ADD COLUMN password_salt TEXT;

-- Generic OAuth identities (replaces provider-specific columns)
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

-- OAuth state tokens (CSRF protection)
CREATE TABLE IF NOT EXISTS oauth_states (
    state      TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    user_id    TEXT
);

-- Refresh tokens (JWT session management)
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email) WHERE email IS NOT NULL;
