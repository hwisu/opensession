# Documentation

OpenSession documentation is organized around real product flows instead of feature lists.
Each chapter follows the same template so functionality and usage examples stay consistent.

## Product Map

### What it does

OpenSession provides a runtime-aware interface for capturing, indexing, and reviewing AI coding sessions.
The same documentation source is exposed as markdown (`Accept: text/markdown`) and rendered HTML (`/docs`).

### How to use

1. Start by checking runtime capabilities from `/api/capabilities`.
2. Use session capture paths (`/upload`, CLI upload, or GitHub preview route) based on your runtime.
3. Move to list and timeline review for search, filtering, and inspection.

### Example

```bash
# Runtime capability check
curl -s https://opensession.io/api/capabilities | jq

# Docs markdown source
curl -H "Accept: text/markdown" https://opensession.io/docs
```

### Limits

- Product map content reflects currently implemented routes only.
- Removed legacy collaboration surfaces are intentionally excluded.

## Capture Sessions

### What it does

Capture flow ingests raw exports and normalizes them into HAIL sessions.
The upload UI supports parser preview, parser selection fallback, and upload confirmation.

### How to use

1. Open `/upload`.
2. Paste or drop a raw session file.
3. Review parser preview results and warnings.
4. Upload the normalized session.

### Example

```bash
# CLI upload to server
opensession publish upload ./session.jsonl

# Session handoff generation
opensession session handoff --last --format json --validate
```

### Limits

- Upload requires `upload_enabled=true` and preview requires `ingest_preview_enabled=true`.
- Worker deployments are typically read-only for upload paths.

## Explore Sessions

### What it does

Explore flow provides searchable list browsing and detailed timeline analysis.
Users can filter by time range, tool, and event categories, then inspect session metadata and event details.

### How to use

1. Open `/` for the session feed.
2. Choose layout mode:
   - `List`: one chronological feed across sessions.
   - `Agents`: grouped by max active agents (parallelism-oriented view).
3. Use list shortcuts:
   - `t` tool, `o` order, `r` range, `l` layout, `/` search.
4. Open a session detail page (`/session/{id}`).
5. Use in-session shortcuts:
   - `/` search focus, `n/p` next/previous match, `1-5` event filter toggles.

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
- List/footer legends are capability-aware and may hide upload-linked actions when upload is disabled.

## GitHub Share Preview

### What it does

GitHub share preview parses source files directly from route parameters:
`/gh/{owner}/{repo}/{ref}/{path...}`.
It supports parser auto-selection, manual parser override, and URL-synced filter state.

### How to use

1. Open a GitHub preview route.
2. If parser selection is required, choose a parser candidate.
3. Switch unified/native views and filters; URL query state updates automatically.

### Example

```bash
# Example route shape
/gh/hwisu/opensession/main/sessions/demo.hail.jsonl

# Optional query controls
/gh/hwisu/opensession/main/sessions/demo.hail.jsonl?view=native&parser_hint=codex
```

### Limits

- Requires `gh_share_enabled=true`.
- Read-only deployments show an unsupported banner instead of preview content.

## Auth & Access

### What it does

Auth flow supports token-based sign-in with automatic account creation on first login (when enabled),
plus optional OAuth providers. Guest users can still access landing and docs.

### How to use

1. Open `/login`.
2. Submit email/password.
3. Use OAuth provider buttons when configured.
4. After login, open the top-right account handle to view linked providers and use logout.
5. Use issued API keys for CLI-to-server integration.

### Example

```bash
# Verify auth capability
curl -s https://opensession.io/api/capabilities | jq '.auth_enabled'

# Connect CLI with issued API key
opensession account connect --server https://opensession.io --api-key <issued_key>
```

### Limits

- Auth endpoints require deployment-side `JWT_SECRET` configuration.
- API keys are shown only at issuance time and are not retrievable later.

## Runtime Profiles

### What it does

Runtime profiles control feature availability without changing route definitions.
Server and worker deployments use the same UI routes but expose different capability flags.

### How to use

1. Query `/api/capabilities`.
2. Verify whether upload, ingest preview, and GitHub share are enabled.
3. Use capability-aware UI states to avoid unsupported flows.

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

- Capability flags are runtime values, not compile-time assumptions.
- Worker profile intentionally disables mutating flows by default.
- Capability-gated E2E tests skip upload/detail flows when required flags are disabled.

## Migration Parity

### What it does

Defines one canonical migration source and one mirror location for numeric remote migrations.

### Canonical and mirror paths

- Canonical: `crates/api/migrations/*.sql`
- Mirror: `migrations/[0-9][0-9][0-9][0-9]_*.sql`

### Commands

```bash
# Validate parity
scripts/check-migration-parity.sh

# Sync mirror from canonical
scripts/sync-migrations.sh
```

## Troubleshooting

### What it does

Troubleshooting guidance helps detect missing capability flags, parser-selection issues, and storage/read-path mismatches quickly.

### How to use

1. Confirm `/api/health` and `/api/capabilities` responses first.
2. Reproduce parser errors with ingest preview endpoints.
3. Verify raw body source behavior using `/api/sessions/{id}/raw`.

### Example

```bash
# Health and capabilities
curl -s https://opensession.io/api/health
curl -s https://opensession.io/api/capabilities

# Migration parity check
scripts/check-migration-parity.sh
```

### Limits

- Troubleshooting assumes environment variables and storage bindings are correctly provisioned.
- Some errors (OAuth provider mismatch, remote storage policies) require deployment configuration changes.
