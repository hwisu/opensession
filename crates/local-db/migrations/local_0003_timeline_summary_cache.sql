-- Timeline summary cache (local-only)
CREATE TABLE IF NOT EXISTS timeline_summary_cache (
    lookup_key  TEXT PRIMARY KEY,
    namespace   TEXT NOT NULL,
    compact     TEXT NOT NULL,
    payload     TEXT NOT NULL,
    raw         TEXT NOT NULL,
    cached_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_timeline_summary_cache_namespace
    ON timeline_summary_cache(namespace);
