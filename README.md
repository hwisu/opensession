# opensession

[![한국어](https://img.shields.io/badge/lang-한국어-blue)](README.ko.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Open-source AI coding session manager. Collect, browse, and share sessions
from Claude Code, Cursor, Codex, Goose, Aider, and other AI tools.

**Website**: [opensession.io](https://opensession.io)
**GitHub**: [github.com/hwisu/opensession](https://github.com/hwisu/opensession)

## Quick Start

### CLI / TUI

```bash
cargo install opensession

opensession ui                    # Launch TUI
opensession session discover      # Find AI sessions on this machine
opensession publish upload-all    # Upload all discovered sessions
```

### Self-Hosted Server

```bash
docker compose up -d
# → http://localhost:3000
# First registered user becomes admin.
```

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
| `parsers` | Session file parsers for 7 AI tools |
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
| `opensession ui` | Launch TUI |
| `opensession session discover` | List all local AI sessions |
| `opensession session index` | Build local session index |
| `opensession session log` | Show session history (git-log style) |
| `opensession session stats` | Show AI usage statistics |
| `opensession session handoff` | Generate handoff summary for next agent |
| `opensession session diff <a> <b>` | Compare two sessions |
| `opensession session timeline` | Export timeline strings (pipe-friendly) |
| `opensession publish upload <file>` | Upload a session file |
| `opensession publish upload-all` | Discover and upload all sessions |
| `opensession ops daemon start\|stop\|status` | Manage background daemon |
| `opensession ops stream enable\|disable` | Real-time session streaming |
| `opensession ops hooks install\|uninstall` | Manage git hooks |
| `opensession account config` | Show or set configuration |
| `opensession account server status\|verify` | Check server connection |
| `opensession docs completion <shell>` | Generate shell completions |

## Configuration

### CLI Config (`~/.config/opensession/config.toml`)

```bash
opensession account config --server https://opensession.io --api-key osk_xxx --team-id my-team
```

### Daemon Config (`~/.config/opensession/daemon.toml`)

Configurable via TUI settings or direct file editing:

```toml
[daemon]
auto_publish = false
publish_on = "manual"        # session_end | realtime | manual
debounce_secs = 5

[server]
url = "https://opensession.io"
api_key = ""

[identity]
nickname = "user"
team_id = ""

[watchers]
claude_code = true
opencode = true
goose = true
aider = true
cursor = false

[privacy]
strip_paths = true
strip_env_vars = true

[git_storage]
method = "none"              # platform_api | native | none
```

### Environment Variables (Server)

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB and session body storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static frontend files |
| `OPENSESSION_BASE_URL` | `http://localhost:3000` | Public-facing URL |
| `JWT_SECRET` | *(required)* | Secret for JWT token signing |
| `PORT` | `3000` | HTTP listen port |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | Log level |

### Local Storage

| Path | Description |
|------|-------------|
| `~/.local/share/opensession/local.db` | Local SQLite cache |
| `~/.config/opensession/config.toml` | CLI configuration |
| `~/.config/opensession/daemon.toml` | Daemon configuration |

## API Endpoints

### Health

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |

### Auth

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/register` | Register (nickname → API key) |
| POST | `/api/auth/register` | Register with email/password |
| POST | `/api/auth/login` | Login with email/password |
| POST | `/api/auth/refresh` | Refresh access token |
| POST | `/api/auth/logout` | Logout (revoke refresh token) |
| POST | `/api/auth/verify` | Verify token validity |
| GET | `/api/auth/me` | Current user settings |
| POST | `/api/auth/regenerate-key` | Generate new API key |
| PUT | `/api/auth/password` | Change password |

### OAuth

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/auth/providers` | List available auth providers |
| GET | `/api/auth/oauth/{provider}` | Redirect to OAuth provider |
| GET | `/api/auth/oauth/{provider}/callback` | OAuth callback |
| POST | `/api/auth/oauth/{provider}/link` | Link OAuth to existing account |

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/sessions` | Upload session (HAIL JSONL body) |
| GET | `/api/sessions` | List sessions (query: `team_id`, `search`, `tool`) |
| GET | `/api/sessions/{id}` | Get session detail |
| DELETE | `/api/sessions/{id}` | Delete session (owner only) |
| GET | `/api/sessions/{id}/raw` | Download raw HAIL file |

### Teams

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/teams` | Create team |
| GET | `/api/teams` | List user's teams |
| GET | `/api/teams/{id}` | Get team detail + sessions |
| PUT | `/api/teams/{id}` | Update team |
| GET | `/api/teams/{id}/stats` | Team usage statistics |
| GET | `/api/teams/{id}/members` | List members |
| POST | `/api/teams/{id}/members` | Add member |
| DELETE | `/api/teams/{id}/members/{user_id}` | Remove member |
| POST | `/api/teams/{id}/invite` | Invite member (email/OAuth) |

### Invitations

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/invitations` | List pending invitations |
| POST | `/api/invitations/{id}/accept` | Accept invitation |
| POST | `/api/invitations/{id}/decline` | Decline invitation |

### Sync

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sync/pull` | Pull sessions (query: `team_id`, `cursor`, `limit`) |

### Docs

| Method | Path | Description |
|--------|------|-------------|
| GET | `/docs` | API documentation |
| GET | `/llms.txt` | LLM-friendly docs |

## Docker

```bash
# Using pre-built image
docker run -p 3000:3000 -v opensession-data:/data \
  -e JWT_SECRET=your-secret-here \
  ghcr.io/hwisu/opensession

# Or with docker compose
docker compose up -d
```

The Docker image is a self-contained monorepo build — no external dependencies required.

## Development

### Prerequisites

- Rust 1.85+
- Node.js 22+ (for frontend)

### Run Locally

```bash
# Server
cargo run -p opensession-server

# Daemon (separate terminal)
cargo run -p opensession-daemon

# TUI
cargo run -p opensession-tui

# Frontend dev server
cd web && npm install && npm run dev
```

### Testing

```bash
cargo test --workspace                        # All workspace tests
cargo test -p opensession-core                # Single crate
cd crates/worker && cargo check --target wasm32-unknown-unknown  # Worker

# Dockerized full web E2E (Playwright, default headless)
scripts/playwright-full-test.sh --rebuild

# Headed mode
scripts/playwright-full-test.sh --headed
```

## HAIL Format

**HAIL** (Human-AI Interaction Log) is an open JSONL format for recording AI coding sessions.

```jsonl
{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"2025-01-01T00:00:00Z"}
{"role":"human","content":"Fix the auth bug"}
{"role":"agent","content":"I'll update...","tool_calls":[...]}
{"type":"file_edit","path":"src/auth.rs","diff":"..."}
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
