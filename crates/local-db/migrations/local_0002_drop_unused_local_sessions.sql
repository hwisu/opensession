-- Local-only cleanup.
-- `local_sessions` is a deprecated cache table that is no longer read by TUI/Daemon.
-- Keep local schema aligned with remote `sessions` + `session_sync` model.

DROP TABLE IF EXISTS local_sessions;
