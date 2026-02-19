# opensession

[![한국어](https://img.shields.io/badge/lang-한국어-blue)](README.ko.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Open-source AI coding session manager. Collect, browse, and share sessions
from Claude Code, Cursor, Codex, Goose, Aider, and other AI tools.

**Website**: [opensession.io](https://opensession.io)  
**GitHub**: [github.com/hwisu/opensession](https://github.com/hwisu/opensession)

## Direction

OpenSession now defaults to a git-native workflow:
- No Docker-required product flow.
- Server profile: auth + session read/upload.
- Worker profile: public session read-only.
- Team/invitation/sync routes are pruned from active runtime paths.

## Quick Start

### CLI

```bash
cargo install opensession

opensession --help
opensession session handoff --last
opensession publish upload-all
opensession daemon start --repo .
```

Manual local browsing mode (TUI):

```bash
opensession      # all local sessions
opensession .    # current git repo scope
```

Optional startup behavior:

```bash
OPS_TUI_REFRESH_DISCOVERY_ON_START=0 opensession
```

When set to `0|false|off|no`, TUI skips full disk re-discovery at startup and relies on cached local DB sessions first.

## Runtime Capabilities

| Area | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| Home `/` | Landing for guests, session list after login | Landing for guests, session list after login |
| Upload UI `/upload` | Enabled | Disabled (read-only) |
| API surface | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` |
| Auth routes | Enabled when `JWT_SECRET` is set | Enabled when `JWT_SECRET` is set |
| Team/invitation/sync routes | Disabled | Disabled |

Web UI behavior is runtime-driven via `GET /api/capabilities` (no build-time profile flag).

## Architecture

```
┌─────────┐    ┌────────┐    ┌──────────────────┐
│  CLI /  │───▶│ daemon │───▶│ server (Axum)    │
│  TUI    │    │ (watch │    │ SQLite + disk     │
└─────────┘    │ +upload)│    │ :3000             │
               └────────┘    └──────────────────┘
```

Single Cargo workspace with 12 crates:

| Crate | Description |
|-------|-------------|
| `core` | HAIL domain model (pure types, validation) |
| `parsers` | Session file parsers for AI tools |
| `api` | Shared API types, SQL builders, service logic |
| `api-client` | HTTP client for server communication |
| `local-db` | Local SQLite database layer |
| `git-native` | Git operations via `gix` |
| `server` | Axum HTTP server with SQLite storage |
| `daemon` | Background file watcher and upload agent |
| `cli` | CLI entry point (binary: `opensession`) |
| `tui` | Terminal UI for browsing sessions |
| `worker` | Cloudflare Workers backend (WASM, excluded from workspace) |
| `e2e` | End-to-end tests |

## CLI Commands

| Command | Description |
|---------|-------------|
| `opensession` / `opensession .` | Launch local interactive mode (all sessions / current repo scope) |
| `opensession session handoff` | Generate handoff summary for next agent |
| `opensession publish upload <file>` | Upload a session file |
| `opensession publish upload-all` | Discover and upload all sessions |
| `opensession publish upload <file> --git` | Store session in git-native branch (`opensession/sessions`) |
| `opensession daemon start\|stop\|status\|health` | Manage daemon lifecycle |
| `opensession daemon select --repo ...` | Select watcher paths/repos |
| `opensession daemon show` | Show daemon watcher targets |
| `opensession account connect` | Set server URL/API key (optional) |
| `opensession account status\|verify` | Check server connectivity |
| `opensession docs completion <shell>` | Generate shell completions |

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
```

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| GET | `/api/capabilities` | Runtime feature flags (`auth_enabled`, `upload_enabled`) |
| GET | `/api/auth/providers` | Available auth providers |
| POST | `/api/auth/register` | Email/password registration |
| POST | `/api/auth/login` | Email/password login |
| POST | `/api/auth/refresh` | Refresh access token |
| POST | `/api/auth/logout` | Revoke refresh token |
| POST | `/api/auth/verify` | Verify access token |
| GET | `/api/auth/me` | Current user profile |
| POST | `/api/sessions` | Upload HAIL session (auth required) |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/{id}` | Get session detail |
| GET | `/api/sessions/{id}/raw` | Download raw HAIL JSONL |
| DELETE | `/api/sessions/{id}` | Delete session |

## Self-Hosted Server

```bash
cargo run -p opensession-server
# -> http://localhost:3000
```

Important environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB and session storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static frontend files |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false` blocks anonymous `GET /api/sessions` |
| `OPENSESSION_SESSION_SCORE_PLUGIN` | `heuristic_v1` | Session score plugin (`heuristic_v1`, `zero_v1`, custom) |
| `PORT` | `3000` | HTTP listen port |

## Migration & DB Parity

Remote migrations must remain byte-identical between:
- `migrations/*.sql`
- `crates/api/migrations/[0-9][0-9][0-9][0-9]_*.sql`

Validation:

```bash
scripts/check-migration-parity.sh
```
