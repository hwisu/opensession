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

# Publish one session (default: server, --git: opensession/sessions branch)
opensession publish upload ./session.jsonl --git

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

TUI/Web Share auth uses personal API keys (no in-TUI email-login setup path):
```bash
opensession account connect --server https://opensession.io --api-key <issued_key>
```
Issue the key from web `/settings`; it is visible only in the issuance response.

## CLI Surface

- `opensession session handoff`
- `opensession session handoff --validate`
- `opensession session handoff --strict`
- `opensession publish upload`
- `opensession daemon start|stop|status|health|select|show|stream-push`
- `opensession account connect|show|status|verify`
- `opensession docs completion`

Notes:
- `publish upload --git` stores sessions on `opensession/sessions` branch.

## Handoff Commands (Verified)

```bash
# Help
cargo run -p opensession -- session handoff --help

# v2 + soft validation gate (exit 0)
cargo run -p opensession -- session handoff --last --format json --validate

# strict validation gate (non-zero on error findings)
cargo run -p opensession -- session handoff --last --validate --strict

# stream envelope output
cargo run -p opensession -- session handoff --last --format stream --validate

# last N sessions (count or HEAD~N)
cargo run -p opensession -- session handoff --last 6 --format json
cargo run -p opensession -- session handoff --last HEAD~6 --format json

# populate HANDOFF.md via provider command
cargo run -p opensession -- session handoff --last 6 --populate claude
cargo run -p opensession -- session handoff --last 6 --populate claude:opus-4.6
```

For repeated runs, use the built binary to avoid `cargo run` startup overhead:
```bash
cargo build -p opensession
target/debug/opensession session handoff --last HEAD~2 --format json
```

CLI-by-CLI examples:

| Source CLI | Example command | Handoff command |
|---|---|---|
| Claude Code | `claude -c` or `claude -p "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --claude HEAD --validate` |
| Codex CLI | `codex exec "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "codex HEAD" --validate` |
| OpenCode | `opencode run "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "opencode HEAD" --validate` |
| Gemini CLI | `gemini -p "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --gemini HEAD --validate` |
| Amp CLI | `amp -x "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "amp HEAD" --validate` |

Tips:
- Use `HEAD~N` instead of `HEAD` for older sessions.
- `--tool "<name> <ref>"` is for tool families without dedicated flags.

Semantics:
- `--validate`: report-only, exit `0`.
- `--validate --strict`: non-zero only on error-level findings.
- default schema is v2.
- `--populate <provider[:model]>`: pipe handoff JSON into provider CLI (`claude`, `codex`, `opencode`, `gemini`, `amp`) and request `HANDOFF.md` population.
- `execution_contract.parallel_actions`: handoff now separates parallelizable work packages from the ordered critical-path list.
- `execution_contract.ordered_steps`: preserves task sequence + timestamps + dependency links for deterministic downstream replay.

## Worker Local Dev (Wrangler, Verified)

```bash
wrangler --version
wrangler dev --help

# basic local run
wrangler dev --ip 127.0.0.1 --port 8788

# preserve local D1/R2 state between runs
wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state

# run on Cloudflare edge
wrangler dev --remote

# debug logs
wrangler dev --ip 127.0.0.1 --port 8788 --log-level debug
```

Notes:
- `wrangler dev` runs `sh build.sh` in this repo.
- Local D1/R2/assets/env bindings are loaded from `wrangler.toml`.
- `--remote` requires Cloudflare auth and can hit live remote resources.

## Configuration

Canonical config file:
- `~/.config/opensession/opensession.toml`

Local cache DB:
- `~/.local/share/opensession/local.db`
- Used as local index/cache (session metadata, sync status, timeline cache), not canonical session body storage.

## Local-DB Scope

- local index/cache responsibilities:
  - `log`, `stats`, `HEAD~N`, sync status, TUI cache load
- default path:
  - v2 handoff output + git-native workflow

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
| `OPENSESSION_DATA_DIR` | `data/` | Server SQLite DB + blob storage |
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
- `GET /api/auth/me` (when `JWT_SECRET` is configured, no API key field)
- `POST /api/auth/api-keys/issue` (when `JWT_SECRET` is configured, key shown once)
- `POST /api/sessions` (server profile, auth required)
- `GET /api/sessions`
- `GET /api/sessions/{id}`
- `GET /api/sessions/{id}/raw`
- `DELETE /api/sessions/{id}`

API keys are intentionally non-retrievable after issuance; `GET /api/auth/me` returns profile only.

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
