# OpenSession Desktop (Preview)

[한국어](README.ko.md)

This is a desktop shell that reuses the existing Svelte web UI from `../web`.

## Run (dev)

```bash
cd desktop
npm install
npm run dev
```

`npm run dev` starts Tauri and the web UI dev server.
It does not require `opensession-server`.

Before running desktop commands, install the repo toolchain via `mise`:

```bash
mise install
```

## Build

```bash
cd desktop
npm run build
```

Build flow:

1. `web` static bundle build (`../web/build`)
2. Tauri desktop bundle

macOS universal bundle (unsigned local verification):

```bash
npm run tauri:build -- --target universal-apple-darwin --bundles app --no-sign --ci
```

## Notes

- UI components are reused from `@opensession/ui` via the existing `web` app.
- In desktop runtime, session/capability/auth reads use Tauri commands backed by local DB and git-native storage.
- Optional: set `OPENSESSION_LOCAL_DB_PATH` to point to a custom sqlite file path.

## Runtime Settings (Desktop Local)

Desktop local runtime exposes runtime settings with a typed summary model:

- `summary.provider.id|endpoint|model`
- `summary.prompt.template`
- `summary.response.style|shape`
- `summary.storage.trigger|backend`
- `summary.source_mode`

Desktop local policy:

- Account/auth section is hidden when `auth_enabled=false` (default desktop local behavior).
- Source mode selector is hidden and internally locked to `session_only`.
- Summary storage backend defaults to `hidden_ref`.
- `hidden_ref` mode still writes searchable list metadata to local SQLite (`local.db`) so filters/search stay fast.
- Response preview is deterministic fixture rendering (no LLM/network dry-run).

Provider field visibility:

- `ollama` (`http`): endpoint + model
- `codex_exec`, `claude_cli` (`cli`): binary status + model
- `disabled`: provider detail fields hidden

Desktop local docs:

- `/docs` can render from local IPC (`desktop_get_docs_markdown`) without requiring `opensession-server`.

Desktop vector search (optional):

- Vector ranking is event/line chunk based (not session-level one-string embedding).
- `vector_search` settings are typed and saved via runtime settings payload.
- Default model is `bge-m3` on local Ollama (`http://127.0.0.1:11434`).
- Model install is explicit from Settings (`desktop_vector_install_model`) and progress is observable via preflight status.
- Indexing is explicit (`desktop_vector_index_rebuild`) and status is queryable (`desktop_vector_index_status`).
- Hidden refs remain summary ledger storage; vector/list metadata remains in local SQLite (`local.db`) for query performance.

## Release

- Product version is synchronized from workspace `Cargo.toml` to desktop files via `scripts/sync-product-version.mjs`.
- Run `node scripts/sync-product-version.mjs --check` before release, or `--write` to apply.
- GitHub Actions `Release` workflow (manual) now runs:
  1. `release-plz update` + release publish
  2. macOS universal Tauri bundle build (`.dmg`, `.app.zip`, checksum)
  3. upload artifacts to tag `v<workspace-version>`
- Universal policy: release build uses `universal-apple-darwin` and validates `lipo -archs` as `x86_64 arm64`.
- Security gate: desktop artifacts are uploaded only when code signing + notarization validation passes.
  Required repo secrets:
  - `APPLE_CERTIFICATE`
  - `APPLE_CERTIFICATE_PASSWORD`
  - `APPLE_SIGNING_IDENTITY`
  - `APPLE_ID`
  - `APPLE_PASSWORD`
  - `APPLE_TEAM_ID`
- Preflight helper for release/CI/local checks:
  - `node scripts/validate/desktop-build-preflight.mjs --mode release --os macos`
