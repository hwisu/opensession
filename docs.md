# Documentation

OpenSession is now optimized for a git-native workflow.

## Runtime Profiles

| Area | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| Primary focus | Read + upload sessions | Public session browsing |
| Home `/` | Guest landing, session list after login | Guest landing, session list after login |
| Upload UI `/upload` | Enabled | Disabled (read-only) |
| Auth routes | Enabled when `JWT_SECRET` is set | Enabled when `JWT_SECRET` is set |
| Team/invitation/sync routes | Disabled | Disabled |
| API surface | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` |

Web UI behavior is runtime-driven via `GET /api/capabilities` (no build-time profile flag).

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

Optional startup behavior:

```bash
OPS_TUI_REFRESH_DISCOVERY_ON_START=0 opensession
```

`0|false|off|no` disables full startup re-discovery and uses cached local DB sessions first.

## CLI Surface

- `opensession session handoff`
- `opensession publish upload`
- `opensession publish upload-all`
- `opensession daemon start|stop|status|health|select|show|stream-push`
- `opensession account connect|show|status|verify`
- `opensession docs completion`

Notes:
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
- `GET /api/capabilities`
- `GET /api/auth/providers`
- `POST /api/auth/register` (when `JWT_SECRET` is configured)
- `POST /api/auth/login` (when `JWT_SECRET` is configured)
- `POST /api/auth/refresh` (when `JWT_SECRET` is configured)
- `POST /api/auth/logout` (when `JWT_SECRET` is configured)
- `POST /api/auth/verify` (when `JWT_SECRET` is configured)
- `GET /api/auth/me` (when `JWT_SECRET` is configured)
- `POST /api/sessions` (server profile, auth required)
- `GET /api/sessions`
- `GET /api/sessions/{id}`
- `GET /api/sessions/{id}/raw`
- `DELETE /api/sessions/{id}`

`GET /api/sessions` supports common query filters:
- `search`
- `tool`
- `sort`
- `time_range`

## Migration Parity

Remote migrations must stay byte-identical between:
- `migrations/*.sql`
- `crates/api/migrations/[0-9][0-9][0-9][0-9]_*.sql`

Validation:

```bash
scripts/check-migration-parity.sh
```
