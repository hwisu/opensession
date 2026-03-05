# Development & Validation Flow

This document is the canonical runbook for local verification and CI parity.
When README/CONTRIBUTING/test docs disagree, this file is the source of truth.

## Required Local Gates

Run both hook stages before pushing:

```bash
./.githooks/pre-commit
./.githooks/pre-push
```

Toolchain requirement:

- Hooks now require `mise` (`mise install` in repo root).
- Hooks execute Node/Rust commands through `mise exec`.
- Desktop build preflight is part of hook validation (`scripts/validate/desktop-build-preflight.mjs`).

`pre-commit` runs:

- content/session-review/workflow/docs guardrail scripts (`node scripts/*.mjs`)
- desktop build preflight (`node scripts/validate/desktop-build-preflight.mjs --mode local`)
- `cargo fmt --all -- --check`
- `cargo test -p opensession-daemon --quiet`

`pre-push` runs:

- full `pre-commit` first
- frontend dependency bootstrap (`npm ci`) with `.opensession/.cache/pre-push` cache
- `packages/ui`: `npm run test --silent`
- `web`: `npm run check`
- `cargo clippy --workspace -- -D warnings`
- worker wasm clippy (`crates/worker`, target `wasm32-unknown-unknown`)
- `cargo test --workspace --exclude opensession-e2e --quiet`
- `cargo test -p opensession-e2e --no-run --quiet`

## E2E Environment Contract

| Variable | Required by | Policy |
| --- | --- | --- |
| `OPENSESSION_E2E_SERVER_BASE_URL` | `crates/e2e` server suite, web live route checks | Required. No default fallback. |
| `OPENSESSION_E2E_WORKER_BASE_URL` | `crates/e2e` worker suite, Playwright live suite | Required. No default fallback. |
| `OPENSESSION_E2E_DESKTOP` | desktop test gates | Set to `1` for desktop E2E mode. |
| `OPENSESSION_E2E_ALLOW_REMOTE` | all E2E surfaces | Default `0`. Remote targets are blocked unless explicitly set to `1`. |

## Local Runtime Validation (Web/Worker + Server)

For web/runtime changes, health-only checks are insufficient. Validate at least one visible route with Playwright live specs.

1. Start worker runtime (`wrangler dev`) in terminal A:

```bash
npx --yes wrangler@4 dev \
  --ip 127.0.0.1 \
  --port 8788 \
  --persist-to .wrangler/state \
  --show-interactive-dev-session=false \
  --log-level=warn \
  --var BASE_URL:http://127.0.0.1:8788 \
  --var OPENSESSION_BASE_URL:http://127.0.0.1:8788 \
  --var JWT_SECRET:local-jwt-secret \
  --var OPENSESSION_ADMIN_KEY:local-admin-key \
  --var GITHUB_CLIENT_ID:local-github-client \
  --var GITHUB_CLIENT_SECRET:local-github-secret
```

2. Start server runtime in terminal B:

```bash
PORT=3000 \
BASE_URL=http://127.0.0.1:3000 \
JWT_SECRET=local-jwt-secret \
OPENSESSION_ADMIN_KEY=local-admin-key \
OPENSESSION_DATA_DIR="$PWD/.ci-data/server" \
OPENSESSION_LOCAL_REVIEW_ROOT="$PWD/web/e2e-live/fixtures/local-review" \
OPENSESSION_ALLOWED_ORIGINS=http://127.0.0.1:8788 \
cargo run -p opensession-server
```

3. Run live E2E in terminal C:

```bash
cd web
OPENSESSION_E2E_WORKER_BASE_URL=http://127.0.0.1:8788 \
OPENSESSION_E2E_SERVER_BASE_URL=http://127.0.0.1:3000 \
OPENSESSION_E2E_ALLOW_REMOTE=0 \
CI=1 \
npm run test:e2e:live -- --reporter=list
```

## Local API/Desktop E2E Shortcuts

Server API E2E:

```bash
OPENSESSION_E2E_SERVER_BASE_URL=http://127.0.0.1:3000 \
OPENSESSION_E2E_ALLOW_REMOTE=0 \
cargo test -p opensession-e2e --test server -- --nocapture
```

Worker API E2E:

```bash
OPENSESSION_E2E_WORKER_BASE_URL=http://127.0.0.1:8788 \
OPENSESSION_E2E_ALLOW_REMOTE=0 \
cargo test -p opensession-e2e --test worker -- --nocapture
```

Desktop E2E:

- Linux (headless): `OPENSESSION_E2E_DESKTOP=1 xvfb-run -a cargo test --manifest-path desktop/src-tauri/Cargo.toml --quiet`
- macOS (native): `OPENSESSION_E2E_DESKTOP=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml --quiet`

## CI Required Gates

`.github/workflows/ci.yml` runs required jobs on both `ubuntu-latest` and `macos-latest`:

- workspace `clippy`
- workspace tests (`opensession-e2e` excluded in unit/integration stage)
- worker wasm clippy
- frontend checks
- API E2E server suite
- worker API + web live Playwright E2E
- desktop E2E
- desktop bundle verify (Linux + macOS universal build + smoke + diagnostics)

Failure artifacts (Playwright report/trace and runtime logs) are uploaded on failing E2E jobs.

Desktop dry-run reliability workflow:

- `.github/workflows/desktop-dryrun.yml` runs on schedule/manual trigger.
- Performs Linux/macOS no-sign bundle builds and smoke checks.
- Uploads diagnostics (`.ci-logs`, `.ci-diagnostics`) and metrics summary artifacts.

## Session Review Automation Output

`session-review` workflow builds a sticky PR comment and optional final snapshot comment.

- Script: `scripts/pr_session_report.mjs`
- Workflow: `.github/workflows/session-review.yml`

Reviewer block highlights:

- `Reviewer Quick Digest`
- Q&A excerpts rendered as `Question | Answer` table rows (content, not counts)
- modified file summary
- added/updated test file summary
- direct local review deep-link (`/review/local/:id`) and commit/session trail
