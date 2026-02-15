-- Team invite keys for email-less onboarding.
-- Keys are stored as SHA-256 hashes and the plaintext is only shown once at creation.

CREATE TABLE IF NOT EXISTS team_invite_keys (
    id         TEXT PRIMARY KEY,
    team_id    TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    key_hash   TEXT NOT NULL UNIQUE,
    role       TEXT NOT NULL DEFAULT 'member',
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    used_by    TEXT REFERENCES users(id),
    used_at    TEXT,
    revoked_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_team_invite_keys_team_id ON team_invite_keys(team_id);
CREATE INDEX IF NOT EXISTS idx_team_invite_keys_expires_at ON team_invite_keys(expires_at);
