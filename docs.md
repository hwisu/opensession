# Documentation

OpenSession turns AI coding traces into public, reviewable session artifacts.
This guide is intentionally scoped to functionality that exists in this repository today.

## Product Overview

OpenSession is currently built around three concrete capabilities.

| Capability | Current state | Primary value |
|---|---|---|
| Browse sessions | Available via `/sessions` and `/session/{id}` | Fast review of real coding workflows |
| Share via git branch | Available via `opensession publish upload <file> --git` | Reproducible, reviewable session artifacts |
| Publish sessions online | Available where upload APIs are enabled | Public session discovery and comparison |

Product goal:
use high-signal public sessions to improve open models and open tooling quality.

### Quick checks

```bash
# Health + capability snapshot
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities | jq

# Docs markdown source
curl -H "Accept: text/markdown" https://opensession.io/docs
```

## Core Capabilities

### Browse Sessions

- Open `/sessions` for the public feed.
- Open `/session/{id}` for timeline detail.
- List shortcuts: `t` tool, `o` order, `r` range, `l` layout, `/` search.
- Detail shortcuts: `/` search, `n/p` next/previous match, `1-5` event filters.

### Share Sessions via Git Branch

- CLI supports git-native share using `--git`.
- Session artifacts are committed to `opensession/sessions` branch flow.
- This makes shared sessions inspectable through normal git history and review tools.

### Publish Sessions Online

- Web upload path: `/upload` (when enabled by runtime capability).
- CLI upload path: `opensession publish upload <file>`.
- Uploaded sessions become available for browsing via `/sessions`.

### Why this matters

- Public session corpora make agent behavior auditable.
- Reproducible trace artifacts improve evaluation quality.
- Open communities can compare workflows and improve prompts/tools collaboratively.

## Runtime Profiles

Capabilities are runtime flags, not one fixed product mode.

| Capability | Server (Axum) | Worker (Wrangler default) |
|---|---|---|
| `auth_enabled` | Depends on `JWT_SECRET` | Depends on `JWT_SECRET` |
| `upload_enabled` | Enabled | Disabled |
| `ingest_preview_enabled` | Enabled | Disabled |
| `gh_share_enabled` | Enabled | Disabled |

Important:
`auth_enabled=true` with upload/preview/share disabled is a valid read-only profile.

## Web Routes

| Route | Purpose | Capability gate |
|---|---|---|
| `/` | Landing (product overview) | none |
| `/sessions` | Session feed | `GET /api/sessions` availability |
| `/session/{id}` | Session detail timeline | `GET /api/sessions/{id}` availability |
| `/upload` | Publish sessions from web | `upload_enabled` + `ingest_preview_enabled` |
| `/gh/{owner}/{repo}/{ref}/{path...}` | Route-based source preview | `gh_share_enabled` |
| `/docs` | Structured docs view | none |
| `/login` | Account sign-in | `auth_enabled` |

## CLI Workflows

```bash
# Upload to server
opensession publish upload ./session.jsonl

# Share via git branch
opensession publish upload ./session.jsonl --git

# Generate handoff from latest session
opensession session handoff --last --format json --validate

# Connect CLI to server account
opensession account connect --server https://opensession.io --api-key <issued_key>
```

## API Summary

| Method | Path | Description |
|---|---|---|
| GET | `/api/health` | Runtime health |
| GET | `/api/capabilities` | Runtime capability flags |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/{id}` | Session detail |
| GET | `/api/sessions/{id}/raw` | Raw HAIL JSONL |
| POST | `/api/sessions` | Publish session (auth/runtime dependent) |
| DELETE | `/api/sessions/{id}` | Delete session (auth/runtime dependent) |
| POST | `/api/ingest/preview` | Parser preview (server profile) |

## Self-Hosting and Verification

```bash
# Server profile
cargo run -p opensession-server

# Worker profile
wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state

# Browser verification (example)
BASE_URL=http://127.0.0.1:8788 npm run test:e2e
```

## Troubleshooting

1. Check health and capabilities first.
2. Match test scenarios to active capability flags.
3. Validate raw session retrieval before parser-level debugging.

```bash
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities | jq
curl -L "https://opensession.io/api/sessions/<id>/raw" | head -n 5
```

If capability combinations look unusual,
verify runtime profile before assuming UI bugs.
