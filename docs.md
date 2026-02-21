# Documentation

OpenSession is an online sharing site for AI coding sessions.
Publish structured traces, review what happened, and keep handoffs reproducible across web, CLI, and TUI.

## Product Overview

OpenSession is built for teams and communities that want to review real AI coding work.

| What you can do | Why it matters | Main surface |
|---|---|---|
| Publish sessions as structured artifacts | Keep full context, not screenshots | `POST /api/sessions`, `opensession publish upload` |
| Review timelines and raw events | Audit tool calls, edits, and outcomes | `/sessions`, `/session/{id}` |
| Share reproducible references | Keep collaboration tied to immutable refs | git refs (`opensession/sessions`) |
| Generate handoffs from real sessions | Preserve execution context between owners | `opensession session handoff` + artifact refs |

Git-based sharing means storing session artifacts on the `opensession/sessions` branch so they can be reviewed and replayed through standard git refs.

### Quick checks

```bash
curl -s https://opensession.io/api/health
curl -H "Accept: text/markdown" https://opensession.io/docs
```

## Web Experience

- Open `/sessions` to browse shared sessions.
- Open `/session/{id}` to inspect the event timeline.
- Open `/git` to preview a session source from `remote/ref/path`.
- Use docs and session review flows to compare workflows with real evidence.

## CLI Workflows

```bash
# Upload one session to the online feed
opensession publish upload ./session.jsonl

# Store one session on git branch sharing flow
opensession publish upload ./session.jsonl --git

# Build and validate handoff from recent sessions
opensession session handoff --last --format json --validate

# Save canonical handoff artifact to git refs
opensession session handoff save --last --payload-format json

# Inspect saved handoff artifacts
opensession session handoff artifact list
```

## TUI Workflows

- Start TUI with `opensession` (or `opensession .` for current repo scope).
- Browse sessions with list filters and quick search (`/`, `t`, `o`, `r`, `l`).
- Open detail view to inspect timeline order, tool usage, and outputs.
- Use the handoff view to generate and save handoff artifacts without leaving terminal.

## Handoff Storage Model

- Canonical handoff storage is git refs: `refs/opensession/handoff/artifacts/<artifact_id>`.
- Artifact payload is structured JSON/JSONL that points back to source sessions.
- `HANDOFF.md` is a derived rendering, not source-of-truth.
- Refresh updates stale artifacts when source sessions change.

## Web Routes

| Route | Purpose | Notes |
|---|---|---|
| `/` | Landing | Product overview for online sharing |
| `/sessions` | Session feed | Public browsing |
| `/session/{id}` | Session detail timeline | Raw event inspection |
| `/git` | Git source preview | Query params: `remote`, `ref`, `path`, optional `parser_hint` |
| `/gh/{owner}/{repo}/{ref}/{path...}` | Compatibility alias | Redirects to `/git` |
| `/docs` | Structured docs view | Markdown-backed chapters |
| `/login` | Account sign-in | Available when auth is enabled |

## API Summary

| Method | Path | Description |
|---|---|---|
| GET | `/api/health` | Runtime health |
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

1. Confirm the session exists and raw data is reachable.
2. Validate `remote/ref/path` input when using `/git` preview.
3. Re-run handoff validation when source sessions changed.

```bash
curl -s https://opensession.io/api/health
curl -L "https://opensession.io/api/sessions/<id>/raw" | head -n 5
opensession session handoff --last --validate
```
