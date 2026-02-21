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
opensession daemon start --repo .
```

Optional stream-write hook path (only when you explicitly integrate agent hook writes):
```bash
opensession daemon enable-hook --agent claude-code
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

TUI/Web Share auth uses personal API keys (not email login inside TUI setup):
```bash
opensession account connect --server https://opensession.io --api-key <issued_key>
```
Issue the key from your web settings page (`/settings`); it is shown once at issuance.

## Runtime Capabilities

| Area | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| Home `/` | Session list (public feed policy applies) | Session list (public feed policy applies) |
| Upload UI `/upload` | Enabled | Disabled (read-only) |
| GitHub share UI `/gh/{owner}/{repo}/{ref}/{path...}` | Enabled | Read-only fallback (unsupported banner) |
| API surface | `/api/health`, `/api/capabilities`, `/api/ingest/preview`, `/api/sessions*`, `/api/auth*` | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` |
| Auth routes | Enabled when `JWT_SECRET` is set | Enabled when `JWT_SECRET` is set |
| Team/invitation/sync routes | Disabled | Disabled |

Web UI behavior is runtime-driven via `GET /api/capabilities` (no build-time profile flag).

## Web UX Map

- Session list layout tabs:
  - `List`: one chronological feed across sessions.
  - `Agents`: grouped by max active agents (parallel lane density view).
- Docs `/docs`:
  - Rendered as chapter cards + TOC sidebar (not a raw markdown dump).
  - Markdown source remains available via `GET /docs` with `Accept: text/markdown`.
- List footer shortcut legend:
  - `t`: cycle tool filter
  - `o`: cycle ordering (`recent`, `popular`, `longest`)
  - `r`: cycle time range (`all`, `24h`, `7d`, `30d`)
  - `l`: toggle list layout (`List`/`Agents`)
- Session detail shortcuts:
  - `/`: focus in-session search, `n/p`: next/previous match, `1-5`: event filters.
- Top-right account handle (`[@handle]`) opens a dropdown with account metadata, linked providers, quick links, and logout.

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
| `local-db` | Local SQLite index/cache layer (metadata, sync state, HEAD refs) |
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
| `opensession session handoff` | Generate immediate v2 execution-contract handoff (`--validate`, `--strict`) |
| `opensession session handoff save ...` | Save merged handoff as canonical git ref artifact (`refs/opensession/handoff/artifacts/<id>`) |
| `opensession session handoff artifact ...` | Artifact lifecycle (`list`, `show`, `refresh`, `render-md`) |
| `opensession publish upload <file> [--git]` | Publish one session (default: server, `--git`: `opensession/sessions` branch) |
| `opensession daemon start\|stop\|status\|health` | Manage daemon lifecycle |
| `opensession daemon enable-hook --agent <name>` | Optional: install stream-write hook for agent-side append workflows |
| `opensession daemon stream-push --agent <name>` | Hook target: stream newly appended local session events |
| `opensession daemon select --repo ...` | Select watcher paths/repos |
| `opensession daemon show` | Show daemon watcher targets |
| `opensession account connect` | Set server URL/API key (optional) |
| `opensession account status\|verify` | Check server connectivity |
| `opensession docs completion <shell>` | Generate shell completions |

Removed command:
- `opensession publish upload-all` was removed; upload sessions explicitly with `opensession publish upload <file>`.

## Handoff Usage (Verified)

Commands verified locally in this repository:

```bash
# Handoff help
cargo run -p opensession -- session handoff --help

# v2 JSON + validation report (soft gate, exit 0)
cargo run -p opensession -- session handoff --last --format json --validate

# Strict validation gate (non-zero on error findings)
cargo run -p opensession -- session handoff --last --validate --strict

# Machine-consumable stream envelope
cargo run -p opensession -- session handoff --last --format stream --validate

# Last N sessions (count or HEAD~N)
cargo run -p opensession -- session handoff --last 6 --format json
cargo run -p opensession -- session handoff --last HEAD~6 --format json

# Populate HANDOFF.md through provider command
cargo run -p opensession -- session handoff --last 6 --populate claude
cargo run -p opensession -- session handoff --last 6 --populate claude:opus-4.6

# Save canonical handoff artifact refs
cargo run -p opensession -- session handoff save --last 6 --payload-format jsonl
cargo run -p opensession -- session handoff artifact list
cargo run -p opensession -- session handoff artifact show <artifact_id>
cargo run -p opensession -- session handoff artifact refresh <artifact_id>
cargo run -p opensession -- session handoff artifact render-md <artifact_id> --output HANDOFF.md
```

For faster repeated runs, prefer the built binary over `cargo run`:
```bash
cargo build -p opensession
target/debug/opensession session handoff --last HEAD~2 --format json
```

Source CLI examples (session creation) and matching handoff commands:

| Source CLI | Example command | Handoff command |
|---|---|---|
| Claude Code | `claude -c` or `claude -p "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --claude HEAD --validate` |
| Codex CLI | `codex exec "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "codex HEAD" --validate` |
| OpenCode | `opencode run "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "opencode HEAD" --validate` |
| Gemini CLI | `gemini -p "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --gemini HEAD --validate` |
| Amp CLI | `amp -x "Fix failing tests and add regression coverage"` | `cargo run -p opensession -- session handoff --tool "amp HEAD" --validate` |

Reference tips:
- Replace `HEAD` with `HEAD~N` to target older sessions.
- `--tool "<name> <ref>"` works for tool families without dedicated flags.

Behavior summary:
- `--validate`: prints human + JSON validation report, exits `0`.
- `--validate --strict`: exits non-zero only for error-level findings.
- default schema is v2 execution-contract output.
- canonical handoff source-of-truth is git ref artifact (`refs/opensession/handoff/artifacts/<artifact_id>`).
- `HANDOFF.md` is a derived output (`artifact render-md`).
- merge policy for artifacts is fixed to chronological ascending (`time_asc`).
- source changes mark artifacts stale; refresh is manual via `artifact refresh`.
- `--populate <provider[:model]>`: pipes handoff JSON to a provider CLI (`claude`, `codex`, `opencode`, `gemini`, `amp`) and asks it to draft `HANDOFF.md`.
- `execution_contract.parallel_actions`: parallelizable work packages are emitted separately from ordered next actions.
- `execution_contract.ordered_steps`: preserves task order + timestamps + dependency links so downstream agents can replay without losing temporal consistency.

## Worker Local Dev (Wrangler, Verified)

```bash
wrangler --version
wrangler dev --help

# basic local run
wrangler dev --ip 127.0.0.1 --port 8788

# preserve local D1/R2 state between runs
wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state

# run on Cloudflare edge (remote resources)
wrangler dev --remote

# debug logs
wrangler dev --ip 127.0.0.1 --port 8788 --log-level debug
```

Notes:
- `wrangler dev` uses `sh build.sh` in this repo and serves the Worker locally.
- Local bindings (D1/R2/assets/env vars) are attached from `wrangler.toml`.
- `--remote` requires Cloudflare auth and can hit real remote resources.
- Worker profile commonly reports `upload_enabled=true` and `ingest_preview_enabled=false`; capability-gated E2E cases should account for ingest preview availability.

## Configuration

Daemon hook policy:
- `opensession daemon start` does not auto-install tool hooks.
- Install hook only when needed for agent stream-write integration:
  - `opensession daemon enable-hook --agent claude-code`
  - `opensession daemon stream-push --agent claude-code`

Canonical config file:
- `~/.config/opensession/opensession.toml`

Local cache DB:
- `~/.local/share/opensession/local.db`
- Used as local index/cache (session metadata, sync status, timeline cache), not canonical session body storage.

## Local-DB Scope

- `local-db` is used for local index/cache concerns:
  - `log`, `stats`, `HEAD~N` resolution
  - sync state and TUI cache load
- default operating path:
  - v2 handoff schema + git-native workflow

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
| GET | `/api/capabilities` | Runtime feature flags (`auth_enabled`, `upload_enabled`, `ingest_preview_enabled`, `gh_share_enabled`) |
| POST | `/api/ingest/preview` | Parse preview from GitHub source or inline file content |
| GET | `/api/auth/providers` | Available auth providers |
| POST | `/api/auth/register` | Email/password registration |
| POST | `/api/auth/login` | Email/password login |
| POST | `/api/auth/refresh` | Refresh access token |
| POST | `/api/auth/logout` | Revoke refresh token |
| POST | `/api/auth/verify` | Verify access token |
| GET | `/api/auth/me` | Current user profile (no API key field) |
| POST | `/api/auth/api-keys/issue` | Issue a new personal API key (shown once) |
| POST | `/api/sessions` | Upload HAIL session (auth required) |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/{id}` | Get session detail |
| GET | `/api/sessions/{id}/raw` | Download raw HAIL JSONL |
| DELETE | `/api/sessions/{id}` | Delete session |

API keys are visible only in the issue response and are not returned by `GET /api/auth/me`.

## Self-Hosted Server

```bash
cargo run -p opensession-server
# -> http://localhost:3000
```

Important environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENSESSION_DATA_DIR` | `data/` | Server SQLite DB and blob storage |
| `OPENSESSION_WEB_DIR` | `web/build` | Static frontend files |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false` blocks anonymous `GET /api/sessions` |
| `OPENSESSION_SESSION_SCORE_PLUGIN` | `heuristic_v1` | Session score plugin (`heuristic_v1`, `zero_v1`, custom) |
| `PORT` | `3000` | HTTP listen port |

## Migration & DB Parity

Canonical migration source is:
- `crates/api/migrations/*.sql`

Mirror target for deploy/tooling compatibility is:
- `migrations/[0-9][0-9][0-9][0-9]_*.sql` (numeric remote migrations only)

Validation:

```bash
scripts/check-migration-parity.sh
```

Sync mirror from canonical:

```bash
scripts/sync-migrations.sh
```
