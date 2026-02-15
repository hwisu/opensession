# Documentation

OpenSession usage guide for the current runtime split:
- Docker (Axum): team-focused deployment
- Worker (Wrangler): personal-sharing deployment

## Profile Differences (Docker vs Worker)

| Area | Docker (Axum server) | Worker (Wrangler) |
|------|-----------------------|-------------------|
| Primary focus | Team collaboration | Personal sharing |
| Home `/` when signed out | Landing page | Landing page |
| Home `/` when signed in | Session list | Session list |
| Team API (`/api/teams*`, `/api/invitations*`, `/api/sync/pull`) | Enabled | Disabled when `ENABLE_TEAM_API=false` |
| Team UI (`/teams`, `/invitations`) | Enabled | Hidden/blocked |
| Upload mode | Team-target upload | Personal upload (`team_id=personal`) |

Repository defaults:
- `docker-compose.yml`: `OPENSESSION_PUBLIC_FEED_ENABLED=false`
- `wrangler.toml`: `ENABLE_TEAM_API=false`
- Web build profile: `VITE_APP_PROFILE=docker|worker`

## Quick Start

### CLI install

```bash
cargo install opensession
```

### Common commands

```bash
# Session handoff
opensession session handoff --last

# Publish local sessions
opensession publish upload-all

# Start daemon (watch + upload targets)
opensession daemon start --agent claude-code --repo .
```

### Local interactive mode (TUI)

```bash
opensession      # all local sessions
opensession .    # current git repository scope
```

### See all available commands

```bash
opensession --help
```

## CLI Reference

### Top-level

- `opensession session handoff`
- `opensession publish upload`
- `opensession publish upload-all`
- `opensession daemon start|stop|status|health|select|show|stream-push`
- `opensession account connect|team|show|status|verify`
- `opensession docs completion`

### `opensession session handoff`

Generate handoff output for one or more sessions.

```bash
opensession session handoff --last
opensession session handoff --claude HEAD
opensession session handoff session1.jsonl session2.jsonl
opensession session handoff --claude HEAD -o handoff.md
```

Supported output formats:
- `text`
- `markdown`
- `json`
- `jsonl`
- `hail`
- `stream` (NDJSON)

Session references:
- `HEAD` (latest)
- `HEAD~N` (latest N merged)
- `HEAD^N` (Nth most recent)
- `<id>` (session ID prefix)
- `<path>` (session file path)

### `opensession publish upload` / `upload-all`

```bash
opensession publish upload ./session.jsonl
opensession publish upload ./session.jsonl --parent abc123
opensession publish upload ./session.jsonl --git
opensession publish upload-all
```

### `opensession daemon`

```bash
opensession daemon start --agent claude-code --repo .
opensession daemon status
opensession daemon health
opensession daemon show
opensession daemon select --agent claude-code --repo .
opensession daemon stop
```

Internal hook target (normally auto-invoked):

```bash
opensession daemon stream-push --agent claude-code
```

### `opensession account`

```bash
opensession account connect --server https://opensession.io --api-key osk_xxx --team-id my-team
opensession account team --id my-team
opensession account show
opensession account status
opensession account verify
```

### `opensession docs completion`

```bash
opensession docs completion bash >> ~/.bashrc
opensession docs completion zsh >> ~/.zshrc
opensession docs completion fish > ~/.config/fish/completions/opensession.fish
```

## Configuration

Unified config file:
- `~/.config/opensession/opensession.toml`

Local cache DB:
- `~/.local/share/opensession/local.db`

Notes:
- Legacy global config fallbacks (`daemon.toml`, `config.toml`) are no longer used.
- Use `opensession.toml` as the single canonical config.

Example:

```toml
[daemon]
auto_publish = false         # managed by TUI "Daemon Capture" toggle
publish_on = "manual"        # ON => session_end, OFF => manual
debounce_secs = 5

[server]
url = "https://opensession.io"
api_key = ""

[identity]
nickname = "user"
team_id = ""

[watchers]
custom_paths = [
  "~/.claude/projects",
  "~/.codex/sessions",
  "~/.local/share/opencode/storage/session",
  "~/.cline/data/tasks",
  "~/.local/share/amp/threads",
  "~/.gemini/tmp",
  "~/Library/Application Support/Cursor/User",
  "~/.config/Cursor/User",
]
```

Legacy per-agent watcher toggles are still accepted when reading old files,
but new writes only persist `watchers.custom_paths`.

## Self-Hosting (Docker)

```bash
docker compose up -d
# -> http://localhost:3000
```

Important environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `JWT_SECRET` | *(required)* | JWT signing secret |
| `OPENSESSION_DATA_DIR` | `data/` | SQLite + session body storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static web directory |
| `BASE_URL` | `http://localhost:3000` | Public URL (OAuth callback base) |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false` blocks anonymous `GET /api/sessions` |
| `PORT` | `3000` | HTTP listen port |

## Worker Deployment (Wrangler)

`wrangler.toml` defaults for personal-sharing profile:
- `ENABLE_TEAM_API=false`
- `BASE_URL=https://opensession.io` (example)

Build profile should match deployment target:

```bash
VITE_APP_PROFILE=worker
```

## API Surface Summary

Always available:
- `/api/health`
- `/api/auth/*`
- `/api/sessions*`

Docker-focused endpoints (team workflows):
- `/api/teams*`
- `/api/invitations*`
- `/api/sync/pull`

Worker with `ENABLE_TEAM_API=false`:
- Team/invitation/sync-team routes are not registered (404)

## HAIL Format

HAIL is line-oriented JSON (`.jsonl`) for AI coding sessions.

```jsonl
{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"2025-01-01T00:00:00Z"}
{"role":"human","content":"Fix the auth bug"}
{"role":"agent","content":"I'll update...","tool_calls":[...]}
{"type":"file_edit","path":"src/auth.rs","diff":"..."}
```

Why JSONL:
- Streamable
- Append-only friendly
- Easy to process with `jq`, `grep`, `wc -l`
