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
- Legacy shortcut routes (`/git`, `/gh/*`, `/resolve/*`) are no longer served and return 404 by design
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

## Install

```bash
cargo install opensession
```

`opensession` is the user-facing CLI. Session auto-capture additionally requires the daemon process to be running.

## Install-and-Forget Setup

```bash
# 1) Install CLI
cargo install opensession

# 2) Diagnose local setup (flutter doctor style)
opensession doctor

# 3) Apply recommended setup values (explicit confirmation prompt)
opensession doctor --fix

# Optional: pin fanout storage mode while fixing
opensession doctor --fix --fanout-mode hidden_ref

# Automation / non-interactive mode
opensession doctor --fix --yes --fanout-mode hidden_ref
```

`doctor` reuses the existing setup pipeline under the hood.
`doctor --fix` now prints the setup plan and asks for confirmation before applying hook/shim/fanout changes.
On first interactive apply, OpenSession asks which fanout storage mode to use (`hidden_ref` or `git_notes`) and stores the choice in local git config (`.git/config`) as `opensession.fanout-mode`.
In non-interactive mode, `--fix` requires `--yes` and an explicit `--fanout-mode` when no fanout mode is already configured in git.

Start daemon (required for automatic session capture):

```bash
# if opensession-daemon binary is available
opensession-daemon run

# from source checkout
cargo run -p opensession-daemon -- run
```

Without daemon, parse/register/share still work manually, but background auto-capture is not active.

## Desktop Preview (Tauri)

A desktop preview shell is available in [`desktop/`](desktop/README.md), reusing the existing Svelte UI.

```bash
cd desktop
npm install
npm run dev
```

This starts both `opensession-server` and the Tauri desktop window.

Desktop release is manual via GitHub Actions `Release` workflow; it now publishes crates and uploads macOS desktop artifacts on the same version tag.

## Quick Start

```bash
# Print the first-user command flow
opensession docs quickstart

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

# Install/update OpenSession pre-push hook (best-effort fanout)
opensession doctor
opensession doctor --fix
# Optional: fail push when fanout is unavailable/fails
OPENSESSION_STRICT=1 git push

# Web URL generation from remote-resolvable URI
opensession config init --base-url https://opensession.io
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

`share --web` requires explicit `.opensession/config.toml`.
Git-native writes now target hidden ledger refs (`refs/opensession/branches/<branch_b64url>`); legacy fixed ref writes are removed.
`opensession doctor --fix` installs the shim at `~/.local/share/opensession/bin/opensession` for hook stability.

## Cleanup Automation

Configure hidden-ref and artifact cleanup for GitHub/GitLab/generic git remotes:

```bash
# initialize cleanup config and templates
opensession cleanup init --provider auto

# non-interactive setup
opensession cleanup init --provider auto --yes

# inspect cleanup status + janitor preview
opensession cleanup status

# dry-run janitor (default)
opensession cleanup run

# apply cleanup deletions
opensession cleanup run --apply
```

Defaults:

- hidden ref TTL: 30 days
- artifact branch TTL: 30 days
- GitHub/GitLab setup also writes PR/MR session-review automation that updates an artifact branch and posts a review comment on PR/MR updates.

Sensitive repositories can force immediate cleanup:

```bash
opensession cleanup init --provider auto --hidden-ttl-days 0 --artifact-ttl-days 0 --yes
```

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

## Failure Recovery

Common failure signatures and immediate recovery commands:

1. `share --web` with a local URI:
```bash
opensession share os://src/local/<sha256> --git --remote origin
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```
2. `share --git` without remote:
```bash
opensession share os://src/local/<sha256> --git --remote origin
```
3. `share --git` outside a git repository:
```bash
cd <your-repo>
opensession share os://src/local/<sha256> --git --remote origin
```
4. `share --web` without `.opensession/config.toml`:
```bash
opensession config init --base-url https://opensession.io
opensession config show
```
5. `register` with non-canonical input:
```bash
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
```
6. `parse` with parser/input mismatch:
```bash
opensession parse --help
opensession parse --profile codex ./raw-session.jsonl --preview
```
7. `view` target cannot be resolved:
```bash
opensession view os://src/... --no-open
opensession view ./session.hail.jsonl --no-open
opensession view HEAD
```
8. `cleanup run` before cleanup setup:
```bash
opensession cleanup init --provider auto
opensession cleanup run
```

5-minute recovery path for first-time users:
```bash
opensession doctor
opensession doctor --fix
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
opensession share os://src/local/<sha256> --git --remote origin
```

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
