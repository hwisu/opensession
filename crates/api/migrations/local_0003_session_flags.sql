-- Local-only session flags.
-- Adds canonical auxiliary-session visibility marker to local cache/index DB.

ALTER TABLE sessions ADD COLUMN is_auxiliary INTEGER NOT NULL DEFAULT 0;

-- Backfill rows that can be identified with SQL-only signals.
UPDATE sessions
SET is_auxiliary = 1
WHERE id IN (
    SELECT s.id
    FROM sessions s
    LEFT JOIN session_sync ss ON ss.session_id = s.id
    WHERE (
            LOWER(COALESCE(ss.source_path, '')) LIKE '%subagents%'
        )
        OR (
            s.tool = 'opencode'
            AND COALESCE(s.user_message_count, 0) <= 0
            AND COALESCE(s.message_count, 0) <= 4
            AND COALESCE(s.task_count, 0) <= 4
            AND COALESCE(s.event_count, 0) > 0
            AND COALESCE(s.event_count, 0) <= 16
        )
);
