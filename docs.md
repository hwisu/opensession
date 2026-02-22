# Documentation

OpenSession is a local-first workflow for registering, sharing, and inspecting AI session traces.
The public contract is a single Source URI model shared by CLI, Web, and API.

## Getting Started

Core principles:

- One concept, one name.
- One identity, one URI.
- No implicit network mutation.
- Defaults are allowed only when printed in output.

Quick path:

```bash
# 1) Parse agent-native logs into canonical HAIL JSONL
opensession parse --profile codex ./raw-session.jsonl > ./session.hail.jsonl

# 2) Register canonical session in local object store
opensession register ./session.hail.jsonl
# -> os://src/local/<sha256>

# 3) Read local canonical bytes back
opensession cat os://src/local/<sha256>

# 4) Inspect summary metadata
opensession inspect os://src/local/<sha256>
```

Local object storage:

- In repo: `.opensession/objects/sha256/ab/cd/<hash>.jsonl`
- Outside repo: `~/.local/share/opensession/objects/sha256/ab/cd/<hash>.jsonl`

Hash policy:

- SHA-256 of canonical HAIL JSONL bytes.

## Share via Git

`register` is local-only. Remote sharing is explicit via `share`.

```bash
# Local source -> remote-shareable source URI
opensession share os://src/local/<sha256> --git --remote origin

# Optional network side effect
opensession share os://src/local/<sha256> --git --remote origin --push
```

`share --git` rules:

- Required: `--remote <name|url>`
- Default ref: `refs/heads/opensession/sessions`
- Default path: `sessions/<sha256>.jsonl`
- `--push` omitted: no network mutation (prints runnable push command)

`share --web` rules:

```bash
opensession config init --base-url https://opensession.io
opensession config show
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

- `share --web` requires explicit `.opensession/config.toml`
- Local URI with `--web` is rejected with follow-up action (`share --git`)
- Human output prints canonical URL as first line

## Inspect Timeline

Canonical web routes:

- `/src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `/src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `/src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`

Legacy routes are removed:

- `/git`
- `/gh/*`
- `/resolve/*`

Server parse-preview endpoint:

- `POST /api/parse/preview`

## Handoff

Handoff artifacts are immutable. Build creates a new artifact URI every time.

```bash
# Build immutable artifact
opensession handoff build --from os://src/local/<sha256> --pin latest
# -> os://artifact/<sha256>

# Read payload representation
opensession handoff artifacts get os://artifact/<sha256> --format canonical --encode jsonl

# Verify deterministic hash
opensession handoff artifacts verify os://artifact/<sha256>

# Alias control
opensession handoff artifacts pin latest os://artifact/<sha256>
opensession handoff artifacts unpin latest

# Removal policy (unpinned only)
opensession handoff artifacts rm os://artifact/<sha256>
```

No refresh/update command exists in v1. Rebuild and move pin aliases.

## Optional UI

CLI is the canonical operator surface.
Web and TUI are optional interfaces over the same URI contract.

## Concepts

Source and artifact identifiers:

- `os://src/local/<sha256>`
- `os://src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`
- `os://artifact/<sha256>`

Encoding rules:

- `ref_enc`: RFC3986 percent-encoding
- `project_b64`, `remote_b64`: base64url (no padding)

API boundary:

- `DELETE /api/admin/sessions/{id}`
- Header: `X-OpenSession-Admin-Key`
