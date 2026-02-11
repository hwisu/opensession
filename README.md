# opensession

[![한국어](https://img.shields.io/badge/lang-한국어-blue)](README.ko.md)

Self-hosted server for AI coding session management. Collect, browse, and share sessions
from Claude Code, Cursor, Codex, and other AI tools.

## Quick Start

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

Three workspace crates:

| Crate | Binary | Description |
|-------|--------|-------------|
| `server` | `opensession-server` | Axum HTTP server with SQLite storage |
| `daemon` | `opensession-daemon` | Background file watcher and sync agent |
| `worker` | *(separate build)* | Cloudflare Workers backend (WASM target) |

> The CLI, TUI, parsers, and core types live in the
> [opensession-core](https://github.com/hwisu/opensession-core) repository.

## Configuration

### Environment Variables (Server)

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB and session body storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static frontend files |
| `OPENSESSION_BASE_URL` | `http://localhost:3000` | Public-facing URL |
| `PORT` | `3000` | HTTP listen port |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | Log level (tracing) |

### Local Storage (no server)

Even without a server, the TUI and daemon store session metadata locally:

| Path | Description |
|------|-------------|
| `~/.local/share/opensession/local.db` | Local SQLite cache (session metadata, git context) |
| `~/.config/opensession/config.toml` | CLI configuration (server URL, API key, team ID) |
| `~/.config/opensession/daemon.toml` | Daemon configuration (server, identity, watchers) |

### Docker Compose

The included `docker-compose.yml` provides:

- **Port**: `3000:3000`
- **Volume**: `opensession-data` → `/data` (SQLite DB + session files)
- **Healthcheck**: `curl -f http://localhost:3000/api/health` every 30s
- **Restart**: `unless-stopped`

The Docker build requires the `opensession-core` repository as an adjacent directory:

```
parent/
├── opensession/          # this repo
└── opensession-core/     # required at build time
```

## API Endpoints

### Health

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |

### Auth

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/register` | Register (nickname → API key) |
| POST | `/api/auth/verify` | Verify token validity |
| GET | `/api/auth/me` | Current user settings |
| POST | `/api/auth/regenerate-key` | Generate new API key |

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/sessions` | Upload session (HAIL JSONL body) |
| GET | `/api/sessions` | List sessions (query: `team_id`, `search`, `tool`) |
| GET | `/api/sessions/{id}` | Get session metadata |
| GET | `/api/sessions/{id}/raw` | Download raw HAIL file |

### Teams

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/teams` | Create team |
| GET | `/api/teams` | List user's teams |
| GET | `/api/teams/{id}` | Get team detail + sessions |
| PUT | `/api/teams/{id}` | Update team |
| GET | `/api/teams/{id}/members` | List members |
| POST | `/api/teams/{id}/members` | Add member |
| DELETE | `/api/teams/{team_id}/members/{user_id}` | Remove member |

### Sync

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/sync/pull` | Pull sessions (query: `team_id`, `since`, `limit`) |

## Development

### Prerequisites

- Rust 1.83+
- Node.js 22+ (for frontend build)

### Run Locally

```bash
# Server
cargo run -p opensession-server

# Daemon (in another terminal)
cargo run -p opensession-daemon
```

### Frontend

```bash
cd web && npm install && npm run dev
```

## License

MIT
