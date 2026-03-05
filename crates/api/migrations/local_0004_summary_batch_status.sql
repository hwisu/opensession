-- Desktop summary batch progress/status snapshot
CREATE TABLE IF NOT EXISTS summary_batch_jobs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    status             TEXT NOT NULL,
    processed_sessions INTEGER NOT NULL DEFAULT 0,
    total_sessions     INTEGER NOT NULL DEFAULT 0,
    failed_sessions    INTEGER NOT NULL DEFAULT 0,
    message            TEXT,
    started_at         TEXT,
    finished_at        TEXT,
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_summary_batch_jobs_status
    ON summary_batch_jobs(status, updated_at DESC);
