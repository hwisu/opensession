# Documentation

OpenSession is a local-first workflow for registering, sharing, and inspecting AI session traces.
The public contract is a single Source URI model shared by CLI, Web, and API.

## Documentation Map

- Root quick reference: `README.md` / `README.ko.md`
- This file (`docs.md`): product contract and command semantics
- Development and CI parity runbook: `docs/development-validation-flow.md`
- Harness failure loop policy: `docs/harness-auto-improve-loop.md`
- Parser source/reuse boundaries: `docs/parser-source-matrix.md`

## Getting Started

Core principles:

- One concept, one name.
- One identity, one URI.
- No implicit network mutation.
- Defaults are allowed only when printed in output.

Beginner 3-step quick start:

```bash
# Print the first-user command flow
opensession docs quickstart

# 1) Install CLI
cargo install opensession

# 2) Diagnose local setup (flutter doctor style)
opensession doctor

# 3) Apply setup after explicit confirmation prompt
opensession doctor --fix --profile local
```

- `doctor --fix` prints a setup plan and asks before applying hook/shim/fanout changes.
- For automation or non-interactive shells, use explicit mode + approval:
  `opensession doctor --fix --yes --profile local --fanout-mode hidden_ref`

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

Install:

```bash
cargo install opensession
```

Install profiles:

- `local` (default): CLI-local-first path for backup/summary/handoff users
- `app`: desktop-linked path for app users (`opensession doctor --fix --profile app --open-target app`)

Auto-capture note:

- `opensession` covers parse/register/share/handoff.
- Automatic background capture requires the daemon process (`opensession-daemon run`) to be running.

Repository development toolchain:

- Local validation hooks are executed via `mise`.
- Run `mise install` at repo root before `./.githooks/pre-commit` / `./.githooks/pre-push`.
- Desktop preflight gate: `node scripts/validate/desktop-build-preflight.mjs --mode local`.

Local object storage:

- In repo: `.opensession/objects/sha256/ab/cd/<hash>.jsonl`
- Outside repo: `~/.local/share/opensession/objects/sha256/ab/cd/<hash>.jsonl`

Hash policy:

- SHA-256 of canonical HAIL JSONL bytes.

## Desktop Runtime Summary Contract (v3)

Desktop IPC/runtime settings use the typed summary contract:

- `summary.provider.id|endpoint|model`
- `summary.prompt.template`
- `summary.response.style|shape`
- `summary.storage.trigger|backend`
- `summary.source_mode`
- `vector_search.enabled|provider|model|endpoint|granularity|chunk_size_lines|chunk_overlap_lines|top_k_chunks|top_k_sessions`

Desktop local constraints:

- `auth_enabled=false` runtime hides account/auth UI by design.
- `summary.source_mode` is locked to `session_only` in desktop local runtime.
- `session_or_git_changes` is reserved for non-desktop runtime contexts (for example CI/CLI).
- Default summary storage backend is `hidden_ref`.
- Even with `hidden_ref`, list/search metadata and vector-index metadata are indexed in local SQLite (`OPENSESSION_LOCAL_DB_PATH` or default `~/.local/share/opensession/local.db`).
- Runtime response preview UI is deterministic local sample rendering, not model output.

Desktop local extras:

- `/docs` can be resolved from desktop IPC (`desktop_get_docs_markdown`) when HTTP docs route is unavailable.
- Vector search uses event/line chunk indexing and local Ollama embeddings (`bge-m3` default).
- Vector search enablement is explicit: model install must complete first (`desktop_vector_preflight`, `desktop_vector_install_model`).
- Indexing is explicit and observable (`desktop_vector_index_rebuild`, `desktop_vector_index_status`).

## Share via Git

`register` is local-only. Remote sharing is explicit via `share`.

```bash
# Local source -> one-click git share URI flow
opensession share os://src/local/<sha256> --quick

# Optional network side effect
opensession share os://src/local/<sha256> --git --remote origin --push
```

`share --git` / `share --quick` rules:

- `--quick` auto-detects remote (`origin` preferred, single remote fallback)
- `--git` requires explicit `--remote <name|url>`
- Default ref: `refs/opensession/branches/<branch_b64url>`
- Default path: `sessions/<sha256>.jsonl`
- `--push` omitted: no network mutation (prints runnable push command)
- `--quick` asks once before first push, then stores per-repo consent in `.git/config` as `opensession.share.auto-push-consent=true`
- Legacy fixed ref `refs/heads/opensession/sessions` is no longer used for new writes.

Install-and-forget setup:

```bash
opensession doctor
opensession doctor --fix --profile local
# optional explicit mode in interactive shells
opensession doctor --fix --profile local --fanout-mode hidden_ref
# automation/non-interactive
opensession doctor --fix --yes --profile local --fanout-mode hidden_ref --open-target web
```

- `doctor` check mode maps to internal setup checks; `doctor --fix` maps to the internal setup apply flow.
- `doctor --fix` requires explicit approval: interactive prompt by default, `--yes` for automation.
- Installs/updates OpenSession-managed `pre-push` hook in the current repo.
- Installs/updates OpenSession shim at `~/.local/share/opensession/bin/opensession`.
- On first apply without a configured mode, interactive shells prompt for fanout mode (`hidden_ref` or `git_notes`) and store it in local git config (`opensession.fanout-mode`).
- Non-interactive apply requires explicit fanout mode (`--fanout-mode`) if the repository has no stored `opensession.fanout-mode`.
- Open target defaults by profile (`local -> web`, `app -> app`).
- `doctor` check output includes daemon status from `~/.config/opensession/daemon.pid`.
- Start daemon with `opensession-daemon run` (or `cargo run -p opensession-daemon -- run` in a source checkout).
- Does **not** modify `remote.<name>.push`.
- Hook fanout push is best-effort and warning-only.
- Set `OPENSESSION_STRICT=1` to fail push when fanout helper is unavailable or fanout push fails.
- PR automation currently targets same-repo non-bot PRs only.
- Merge/branch-delete cleanup removes ledger refs immediately; physical object removal follows remote GC policy.

`share --web` rules:

```bash
opensession config init --base-url https://opensession.io
opensession config show
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

- `share --web` requires explicit `.opensession/config.toml`
- Local URI with `--web` is rejected with follow-up action (`share --git`)
- Human output prints canonical URL as first line

## Cleanup Automation

OpenSession can configure hidden ref cleanup for user repositories without changing server-side infrastructure.

```bash
# Initialize provider-aware cleanup templates/config
opensession cleanup init --provider auto

# Non-interactive setup
opensession cleanup init --provider auto --yes

# Inspect config + janitor dry-run summary
opensession cleanup status

# Dry-run (default)
opensession cleanup run

# Apply deletions
opensession cleanup run --apply

# Keep review snapshots permanently on a dedicated branch
opensession cleanup init --provider auto --session-archive-branch pr/sessions --yes
```

Defaults:

- hidden ref TTL: 30 days
- artifact branch TTL: 30 days

For sensitive repositories:

```bash
opensession cleanup init --provider auto --hidden-ttl-days 0 --artifact-ttl-days 0 --yes
```

Provider matrix:

- GitHub: `.github/workflows/opensession-cleanup.yml` plus `.github/workflows/opensession-session-review.yml` are generated. By default PR updates publish ephemeral `opensession/pr-<number>-sessions` branches and delete them when the PR closes; set `--session-archive-branch <branch>` to keep immutable review snapshots on a dedicated archive branch such as `pr/sessions`.
- GitLab: `.gitlab/opensession-cleanup.yml` plus `.gitlab/opensession-session-review.yml` are generated; `.gitlab-ci.yml` is updated only when an OpenSession managed marker block exists (or file is newly created). MR pipelines publish/refresh `opensession/mr-<iid>-sessions` and post an MR note, or use the configured archive branch when `--session-archive-branch` is set.
- Generic git: `.opensession/cleanup/cron.example` is generated for cron/system scheduler wiring.
- Session-review comments include `Reviewer Quick Digest` with mobile-friendly Q&A prose, modified file summary, and added/updated tests.

## Development & Validation

Canonical validation flow (hooks, API/worker/web/desktop E2E, CI parity, artifact policy):

- `docs/development-validation-flow.md`

Quick local gate commands:

```bash
./.githooks/pre-commit
./.githooks/pre-push
```

GitHub CI split:

- `.github/workflows/ci.yml` keeps fast PR/main gates only.
- `.github/workflows/ci-deep.yml` owns long-running audit/E2E/desktop validation on schedule or manual trigger.

Desktop build policy:

- Linux desktop bundle build verification is required in deep CI (`desktop-bundle-verify`).
- macOS desktop release target is `universal-apple-darwin` only.
- Universal architecture is validated by `lipo -archs` and must include both `x86_64` and `arm64`.
- A scheduled/manual `Desktop Dry Run` workflow validates no-sign desktop bundling and uploads diagnostics/metrics artifacts.

Release signing checklist (manual secret provisioning):

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

## Failure Recovery

Use these commands when a common onboarding flow fails:

1. `share --web` got a local URI:
```bash
opensession share os://src/local/<sha256> --git --remote origin
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```
2. `share --git` missing `--remote`:
```bash
opensession share os://src/local/<sha256> --quick
```
3. `share --git` outside git repo:
```bash
cd <repo>
opensession share os://src/local/<sha256> --quick
```
4. `share --web` missing config:
```bash
opensession config init --base-url https://opensession.io
opensession config show
```
5. `register` rejected non-canonical input:
```bash
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
```
6. `parse` parser/input mismatch:
```bash
opensession parse --help
opensession parse --profile codex ./raw-session.jsonl --preview
```
7. `view` target resolution failed:
```bash
opensession view os://src/... --no-open
opensession view ./session.hail.jsonl --no-open
opensession view HEAD
```
8. `cleanup run` before initialization:
```bash
opensession cleanup init --provider auto
opensession cleanup run
```

Five-minute first-user recovery path:
```bash
opensession doctor
opensession doctor --fix --profile local
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
opensession share os://src/local/<sha256> --quick
```

## Inspect Timeline

Canonical web routes:

- `/src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `/src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `/src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`

Legacy shortcut routes are reserved and return 404:

- `/git`
- `/gh/*`
- `/resolve/*`

Server parse-preview endpoint:

- `POST /api/parse/preview`

## Review View

`opensession view` is the review-first entrypoint for web view.

```bash
# Source URI -> /src/*
opensession view os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>

# Local source URI / jsonl file -> /review/local/<id>
opensession view os://src/local/<sha256>
opensession view ./session.hail.jsonl

# Commit/ref/range -> commit-linked local review bundle
opensession view HEAD
opensession view main..feature/my-branch
```

Default mode is web. Use `--no-open` to print URL only.

Local `view` targets do not require registered git credentials.
They use local git objects / local source bytes and generate a local review bundle.
Commit-linked local review pages expose a `Reviewer Quick Digest` panel that renders mobile-friendly Q&A content excerpts (not just counts), modified files, and added/updated tests.

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
