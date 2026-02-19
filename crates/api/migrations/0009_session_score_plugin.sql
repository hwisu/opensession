ALTER TABLE sessions ADD COLUMN session_score INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN score_plugin TEXT NOT NULL DEFAULT 'heuristic_v1';

CREATE INDEX IF NOT EXISTS idx_sessions_session_score ON sessions(session_score DESC);
