# OpenSession Agent/UI Contracts

## Message Count Contract (`stats.message_count`)
- `message_count` MUST include:
  - every `UserMessage`
  - every `AgentMessage`
  - every `TaskEnd` that has a non-empty `summary`
- `user_message_count` remains user-only.

## Session List Display Contract
- `msgs` label MUST display `message_count`.
- `agents` MUST never render as `0`; minimum is `1 agent`.
- `LIVE` badge is shown when the latest known activity timestamp is within 5 minutes.
- Date formatting follows `calendar_display_mode` (`smart`, `relative`, `absolute`).

## Summary-Off Detail Contract
- When LLM summary is unavailable/off, detail view MUST still render task-level execution visibility.
- Fallback rendering uses task buckets (`main` + `task_id` buckets) with status, counters, and last output.

## Deployment Profile Contract
- Runtime stays Rust-first in this repository (no TS/Go runtime replacement in this phase).
- Docker/Axum profile is team-focused and may disable anonymous session feed via `OPENSESSION_PUBLIC_FEED_ENABLED=false`.
- Worker/Wrangler profile is personal-sharing focused with team APIs disabled via `ENABLE_TEAM_API=false`.
- Web UI behavior must follow `VITE_APP_PROFILE=docker|worker`:
  - `docker`: Teams/Inbox surface enabled.
  - `worker`: Teams/Inbox surface hidden/blocked.
