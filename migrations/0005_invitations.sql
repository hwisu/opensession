-- Team invitations (provider-agnostic)
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
