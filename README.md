# opensession

[![한국어](https://img.shields.io/badge/lang-한국어-blue)](README.ko.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

OpenSession is a local-first workflow for recording, registering, sharing, and inspecting AI session traces.

Website: [opensession.io](https://opensession.io)  
Docs: [opensession.io/docs](https://opensession.io/docs)

## DX Reset v1

The CLI/Web/API contract is now centered on three actions:

- `register`: store canonical HAIL JSONL locally (no network side effects)
- `share`: produce a shareable output from a Source URI
- `handoff`: build immutable artifacts and manage artifact aliases

Legacy command trees and routes were removed:

- `opensession publish ...` removed
- `opensession session handoff ...` removed
- `/git` and `/gh/*` routes removed
- `/api/ingest/preview` removed in favor of `/api/parse/preview`

## URI Model

- `os://src/local/<sha256>`
- `os://src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`
- `os://artifact/<sha256>`

Notes:

- `ref_enc` is RFC3986 percent-encoding.
- `project_b64` / `remote_b64` are base64url without padding.

## Quick Start

```bash
# Parse agent-native log -> canonical HAIL JSONL
opensession parse --profile codex ./raw-session.jsonl > ./session.hail.jsonl

# Register to local object store (repo-scoped by default)
opensession register ./session.hail.jsonl
# -> os://src/local/<sha256>

# Read it back
opensession cat os://src/local/<sha256>

# Inspect summary metadata
opensession inspect os://src/local/<sha256>
```

## Share

```bash
# Convert local URI -> git shareable source URI
opensession share os://src/local/<sha256> --git --remote origin

# Optional network mutation
opensession share os://src/local/<sha256> --git --remote origin --push

# Web URL generation from remote-resolvable URI
opensession config init --base-url https://opensession.io
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

`share --web` requires explicit `.opensession/config.toml`.

## Handoff

```bash
# Build immutable artifact
opensession handoff build --from os://src/local/<sha256> --pin latest
# -> os://artifact/<sha256>

# Read artifact payload in desired representation
opensession handoff artifacts get os://artifact/<sha256> --format canonical --encode jsonl

# Verify hash + payload validity
opensession handoff artifacts verify os://artifact/<sha256>

# Alias management
opensession handoff artifacts pin latest os://artifact/<sha256>
opensession handoff artifacts unpin latest

# Removal policy: only unpinned artifacts
opensession handoff artifacts rm os://artifact/<sha256>
```

## Canonical Web Routes

- `/src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `/src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `/src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`

## API Surface (v1)

- `GET /api/health`
- `GET /api/capabilities`
- `POST /api/parse/preview`
- `GET /api/sessions`
- `GET /api/sessions/{id}`
- `GET /api/sessions/{id}/raw`
- `DELETE /api/admin/sessions/{id}` (requires `X-OpenSession-Admin-Key`)

## Local Development

```bash
# Required validation gates
./.githooks/pre-commit
./.githooks/pre-push
```

```bash
# Runtime web validation
npx wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state
BASE_URL=http://127.0.0.1:8788 npx playwright test e2e/git-share.spec.ts --config playwright.config.ts
```
