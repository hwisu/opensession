ALTER TABLE sessions
ADD COLUMN max_active_agents INTEGER NOT NULL DEFAULT 1;

UPDATE sessions
SET max_active_agents = 1
WHERE max_active_agents IS NULL OR max_active_agents < 1;
