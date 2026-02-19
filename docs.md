# Documentation

OpenSession is now optimized for a git-native workflow.

## Runtime Profiles

| Area | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| Primary focus | Read + upload sessions | Public session browsing |
| Home `/` | Session list | Session list |
| Upload UI `/upload` | Enabled | Disabled (read-only) |
| Team/auth routes | Disabled | Disabled |
| API surface | `/api/health`, `/api/sessions*` | `/api/health`, `/api/sessions*` |

Build profile:
- `VITE_APP_PROFILE=server|worker`

## Quick Start

### CLI install

```bash
cargo install opensession
```

### Common commands

```bash
# Session handoff
opensession session handoff --last

# Upload one session
opensession publish upload ./session.jsonl

# Upload all discovered sessions
opensession publish upload-all

# Start daemon (watch + upload)
opensession daemon start --repo .
```

### Local interactive mode (TUI)

```bash
opensession      # all local sessions
opensession .    # current git repository scope
```

## CLI Surface

- `opensession session handoff`
- `opensession publish upload`
- `opensession publish upload-all`
- `opensession daemon start|stop|status|health|select|show|stream-push`
- `opensession account connect|show|status|verify|team`
- `opensession docs completion`

Notes:
- `account team` is legacy/optional. When unset, uploads default to `local` scope.
- `publish upload --git` stores sessions on `opensession/sessions` branch.

## Configuration

Canonical config file:
- `~/.config/opensession/opensession.toml`

Local cache DB:
- `~/.local/share/opensession/local.db`

Example:

```toml
[server]
url = "http://localhost:3000"
api_key = ""

[identity]
nickname = "user"
team_id = ""   # optional; empty => local scope

[watchers]
custom_paths = [
  "~/.claude/projects",
  "~/.codex/sessions",
]

[git_storage]
method = "native"  # native | sqlite
```

## Self-Hosting (Server)

```bash
cargo run -p opensession-server
# -> http://localhost:3000
```

Important environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite + session body storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static web directory |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false` blocks anonymous `GET /api/sessions` |
| `OPENSESSION_SESSION_SCORE_PLUGIN` | `heuristic_v1` | Session score plugin id |
| `PORT` | `3000` | HTTP listen port |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | Log level |

## API Summary

Always available:
- `GET /api/health`
- `POST /api/sessions`
- `GET /api/sessions`
- `GET /api/sessions/{id}`
- `GET /api/sessions/{id}/raw`
- `DELETE /api/sessions/{id}`

`GET /api/sessions` supports common query filters:
- `search`
- `tool`
- `sort`
- `time_range`
- `risk_level`
- `triage_status`
- `policy_status`
- `outcome_status`

## Migration Parity

Remote migrations must stay byte-identical between:
- `migrations/*.sql`
- `crates/api/migrations/[0-9][0-9][0-9][0-9]_*.sql`

Validation:

```bash
scripts/check-migration-parity.sh
```
