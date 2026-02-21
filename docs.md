# Documentation

OpenSession turns AI coding traces into durable, reviewable session artifacts.
The product language in this guide is goal-driven so it remains stable across runtime profiles.

## Product Overview

OpenSession is designed around four product goals.

| Goal | Why it matters | Typical surface |
|---|---|---|
| Capture work as structured data | Preserve full context instead of partial screenshots | HAIL session format |
| Review what actually happened | Make tool calls, edits, and outcomes auditable | `/sessions`, `/session/{id}` |
| Share reproducible artifacts | Keep collaboration tied to immutable references | git refs + upload API |
| Improve workflows continuously | Turn real traces into feedback loops | docs, session comparisons, review |

Git-based sharing means storing session artifacts on the `opensession/sessions` branch so they can be reviewed and replayed through standard git refs.

### Quick checks

```bash
# Health + capability snapshot
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities | jq

# Docs markdown source
curl -H "Accept: text/markdown" https://opensession.io/docs
```

## Core Goals

### Capture

- Normalize sessions into HAIL-compatible artifacts.
- Keep one data model across terminal and web views.

### Review

- Open `/sessions` for the public feed.
- Open `/session/{id}` for timeline detail.
- List shortcuts: `t` tool, `o` order, `r` range, `l` layout, `/` search.
- Detail shortcuts: `/` search, `n/p` next/previous match, `1-5` event filters.

### Share

- Upload API accepts session payloads via `POST /api/sessions`.
- Git preview accepts direct source query params via `/git`.
- Legacy `/gh/{owner}/{repo}/{ref}/{path...}` routes are preserved as compatibility redirects.

### Improve

- Compare real workflows and outcomes, not claims.
- Use artifacts to refine prompts, tools, and evaluation loops.

## Runtime Profiles

Capabilities are runtime flags, not separate products.

| Capability | Server (Axum) | Worker (Wrangler default) |
|---|---|---|
| `auth_enabled` | Depends on `JWT_SECRET` | Depends on `JWT_SECRET` |
| `upload_enabled` | Enabled | Enabled |
| `ingest_preview_enabled` | Enabled | Disabled |
| `gh_share_enabled` | Enabled | Disabled |

## Web Routes

| Route | Purpose | Notes |
|---|---|---|
| `/` | Landing | Goal-oriented overview |
| `/sessions` | Session feed | Public browsing |
| `/session/{id}` | Session detail timeline | Raw event inspection |
| `/git` | Git source preview | Query params: `remote`, `ref`, `path`, optional `parser_hint` |
| `/gh/{owner}/{repo}/{ref}/{path...}` | Compatibility alias | Redirects to `/git` |
| `/docs` | Structured docs view | Markdown-backed chapters |
| `/login` | Account sign-in | Available when auth is enabled |

## CLI Workflows

```bash
# Upload to server API
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
| POST | `/api/sessions` | Publish session |
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
2. Validate route inputs (`remote`, `ref`, `path`) for `/git` previews.
3. Validate raw session retrieval before parser-level debugging.

```bash
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities | jq
curl -L "https://opensession.io/api/sessions/<id>/raw" | head -n 5
```
