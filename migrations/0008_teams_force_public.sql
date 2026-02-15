-- Teams are currently modeled as always-public workspaces.
-- Backfill any existing private teams to public.
UPDATE teams
SET is_public = 1
WHERE COALESCE(is_public, 0) = 0;
