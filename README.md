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
- Server profile: session read/upload.
- Worker profile: public session read-only.
- Team/auth routes are pruned from active runtime paths.

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

## Runtime Profiles

| Area | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| Home `/` | Session list | Session list |
| Upload UI `/upload` | Enabled | Disabled (read-only) |
| API surface | `/api/health`, `/api/sessions*` | `/api/health`, `/api/sessions*` |
| Team/auth runtime routes | Disabled | Disabled |

Web build profile:
- `VITE_APP_PROFILE=server|worker`

## Architecture

```
┌─────────┐    ┌────────┐    ┌──────────────────┐
│  CLI /  │───▶│ daemon │───▶│ server (Axum)    │
│  TUI    │    │ (watch │    │ SQLite + disk     │
└─────────┘    │ +sync) │    │ :3000             │
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
| `daemon` | Background file watcher and sync agent |
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
| `opensession account team` | Optional legacy scope id command |
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
team_id = ""  # optional; empty => local scope

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
| POST | `/api/sessions` | Upload HAIL session |
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
