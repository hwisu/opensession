-- API key issuance table: keys are shown once at issue time and persisted as hashes.

CREATE TABLE IF NOT EXISTS api_keys (
    id          TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_hash    TEXT NOT NULL UNIQUE,
    key_prefix  TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    grace_until TEXT,
    revoked_at  TEXT,
    last_used_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user_status ON api_keys(user_id, status);
CREATE INDEX IF NOT EXISTS idx_api_keys_grace_until ON api_keys(grace_until);
