-- Drop legacy team-model schema objects that are no longer used at runtime.
-- Sessions still keep `team_id` for compatibility, but team tables and team-only
-- indexes are removed to avoid stale DB references.

DROP INDEX IF EXISTS idx_sessions_visible_team_created_at;
DROP INDEX IF EXISTS idx_sessions_team_id;

DROP INDEX IF EXISTS idx_team_members_user;
DROP INDEX IF EXISTS idx_team_members_user_id;
DROP INDEX IF EXISTS idx_invitations_team;

DROP TABLE IF EXISTS team_invite_keys;
DROP TABLE IF EXISTS team_members;
DROP TABLE IF EXISTS team_invitations;
DROP TABLE IF EXISTS teams;
