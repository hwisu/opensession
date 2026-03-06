-- Desktop lifecycle cleanup progress/status snapshot
CREATE TABLE IF NOT EXISTS lifecycle_cleanup_jobs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    status            TEXT NOT NULL,
    deleted_sessions  INTEGER NOT NULL DEFAULT 0,
    deleted_summaries INTEGER NOT NULL DEFAULT 0,
    message           TEXT,
    started_at        TEXT,
    finished_at       TEXT,
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_lifecycle_cleanup_jobs_status
    ON lifecycle_cleanup_jobs(status, updated_at DESC);
