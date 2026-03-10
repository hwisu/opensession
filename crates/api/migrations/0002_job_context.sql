ALTER TABLE sessions ADD COLUMN job_protocol TEXT;
ALTER TABLE sessions ADD COLUMN job_system TEXT;
ALTER TABLE sessions ADD COLUMN job_id TEXT;
ALTER TABLE sessions ADD COLUMN job_title TEXT;
ALTER TABLE sessions ADD COLUMN job_run_id TEXT;
ALTER TABLE sessions ADD COLUMN job_attempt INTEGER;
ALTER TABLE sessions ADD COLUMN job_stage TEXT;
ALTER TABLE sessions ADD COLUMN job_review_kind TEXT;
ALTER TABLE sessions ADD COLUMN job_status TEXT;
ALTER TABLE sessions ADD COLUMN job_thread_id TEXT;
ALTER TABLE sessions ADD COLUMN job_artifact_count INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_sessions_job_id_created_at
ON sessions(job_id, created_at DESC)
WHERE job_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sessions_job_run_id_created_at
ON sessions(job_run_id, created_at DESC)
WHERE job_run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sessions_job_review_lookup
ON sessions(job_id, job_review_kind, created_at DESC)
WHERE job_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sessions_job_stage_status_created_at
ON sessions(job_stage, job_status, created_at DESC)
WHERE job_stage IS NOT NULL;
