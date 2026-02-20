# Documentation

OpenSession stores AI coding sessions in HAIL and makes them inspectable through feed + timeline views.
This document is limited to behavior that exists in the current repository and runtime profiles.

## Product Map

### What it does

OpenSession is built around a single practical loop:
capture logs, normalize to HAIL, index metadata, review sessions quickly.
The same docs source is exposed as markdown (`Accept: text/markdown`) and rendered HTML (`/docs`).

### How to use

1. Check runtime state from `/api/capabilities`.
2. Choose available ingestion path (`/upload`, CLI upload, or `/gh/...` preview).
3. Review sessions from `/` and `/session/{id}`.

### Example

```bash
# Runtime capability check
curl -s https://opensession.io/api/capabilities | jq

# Docs markdown source
curl -H "Accept: text/markdown" https://opensession.io/docs
```

### Limits

- This map tracks implemented routes and commands only.
- Capability flags are runtime values, not promises.

## Capture Sessions

### What it does

Capture flow ingests raw exports and normalizes them into HAIL sessions.
Upload UI and CLI upload both feed the same session format.

### How to use

1. Verify `upload_enabled` and `ingest_preview_enabled` from `/api/capabilities`.
2. Open `/upload` and provide a raw file.
3. Confirm parser preview/candidate parser.
4. Upload and verify the session appears in `/`.

### Example

```bash
# CLI upload to server
opensession publish upload ./session.jsonl

# Optional parser preview API check
curl -s -X POST https://opensession.io/api/ingest/preview \
  -H "content-type: application/json" \
  -d '{"inline_source":"{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}"}'
```

### Limits

- Capture requires `upload_enabled=true`; preview requires `ingest_preview_enabled=true`.
- Worker profile is typically read-only for capture flows.

## Explore Sessions

### What it does

Explore flow provides session list browsing and event-level timeline inspection.
`List` means one chronological feed; `Agents` groups by max active agents (parallelism view).

### How to use

1. Open `/` for the session feed.
2. Use list shortcuts: `t` tool, `o` order, `r` range, `l` layout, `/` search.
3. Open `/session/{id}` for timeline detail.
4. Use detail shortcuts: `/` search, `n/p` next/previous match, `1-5` event filter toggles.

### Example

```bash
# List sessions
curl -s "https://opensession.io/api/sessions?per_page=20&page=1"

# Download raw session
curl -L "https://opensession.io/api/sessions/<id>/raw" -o session.hail.jsonl
```

### Limits

- Public feed visibility depends on deployment policy.
- Detail rendering assumes parseable HAIL-compatible event structures.

## GitHub Share Preview

### What it does

GitHub share preview parses source files directly from route parameters:
`/gh/{owner}/{repo}/{ref}/{path...}`.
UI state (view/filter/parser hint) is reflected in URL query params.

### How to use

1. Open a GitHub preview route.
2. If parser confidence is low, pick a parser candidate.
3. Switch unified/native views and filters as needed.

### Example

```bash
# Example route shape
/gh/hwisu/opensession/main/sessions/demo.hail.jsonl

# Optional query controls
/gh/hwisu/opensession/main/sessions/demo.hail.jsonl?view=native&parser_hint=codex
```

### Limits

- Requires `gh_share_enabled=true`.
- Disabled runtimes return explicit unsupported UI state.

## Auth & Access

### What it does

Auth flow provides token-based sign-in and account metadata in the top-right handle dropdown.
Guests can still use landing/docs and public session flows.

### How to use

1. Check `auth_enabled` from `/api/capabilities`.
2. Open `/login` and sign in with email/password.
3. Open the account handle menu to review providers and logout.
4. Issue an API key for CLI-to-server integration when needed.

### Example

```bash
# Verify auth capability
curl -s https://opensession.io/api/capabilities | jq '.auth_enabled'

# Connect CLI with issued API key
opensession account connect --server https://opensession.io --api-key <issued_key>
```

### Limits

- Auth endpoints require `JWT_SECRET` in deployment config.
- API keys are visible only at issuance time.

## Runtime Profiles

### What it does

Server and worker use the same frontend routes but expose different capabilities.
Behavior differences are expected and should be read from `/api/capabilities`.

### How to use

1. Start the runtime you are validating.
2. Read `/api/capabilities` first.
3. Test only flows enabled by that runtime profile.

### Example

```bash
# Worker local dev with persisted state
wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state

# Server local dev
cargo run -p opensession-server

# E2E against worker profile (capability-gated skips are expected)
BASE_URL=http://127.0.0.1:8788 npm run test:e2e
```

### Limits

- Worker profile commonly reports `upload_enabled=false` and `ingest_preview_enabled=false`.
- Capability-gated E2E tests intentionally skip unavailable flows.

## Troubleshooting

### What it does

Troubleshooting focuses on capability mismatches, parser-selection failures, and session read-path issues.

### How to use

1. Confirm `/api/health` and `/api/capabilities`.
2. Reproduce parse failures through ingest preview or `/gh/...` route.
3. Verify raw session retrieval with `/api/sessions/{id}/raw`.

### Example

```bash
# Health and capability checks
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities

# Raw session check
curl -L "https://opensession.io/api/sessions/<id>/raw" | head -n 5
```

### Limits

- Troubleshooting assumes environment variables and storage bindings are configured.
- Some failures require deployment-level config updates rather than code changes.
