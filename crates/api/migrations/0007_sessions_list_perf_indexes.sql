-- Improve session list performance for public feed and filtered views.
-- These indexes target /api/sessions patterns:
-- - recent feed (created_at DESC)
-- - team/tool filtered recent feed
-- - popular/longest sort modes

CREATE INDEX IF NOT EXISTS idx_sessions_visible_created_at
ON sessions(created_at DESC)
WHERE event_count > 0 OR message_count > 0;

CREATE INDEX IF NOT EXISTS idx_sessions_visible_team_created_at
ON sessions(team_id, created_at DESC)
WHERE event_count > 0 OR message_count > 0;

CREATE INDEX IF NOT EXISTS idx_sessions_visible_tool_created_at
ON sessions(tool, created_at DESC)
WHERE event_count > 0 OR message_count > 0;

CREATE INDEX IF NOT EXISTS idx_sessions_visible_popular
ON sessions(message_count DESC, created_at DESC)
WHERE event_count > 0 OR message_count > 0;

CREATE INDEX IF NOT EXISTS idx_sessions_visible_longest
ON sessions(duration_seconds DESC, created_at DESC)
WHERE event_count > 0 OR message_count > 0;
