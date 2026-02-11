CREATE TABLE IF NOT EXISTS session_links (
    session_id        TEXT NOT NULL,
    linked_session_id TEXT NOT NULL,
    link_type         TEXT NOT NULL DEFAULT 'handoff',
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (session_id, linked_session_id)
);
CREATE INDEX IF NOT EXISTS idx_session_links_linked ON session_links(linked_session_id);
