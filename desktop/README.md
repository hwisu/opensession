# OpenSession Desktop (Preview)

This is a desktop shell that reuses the existing Svelte web UI from `../web`.

## Run (dev)

```bash
cd desktop
npm install
npm run dev
```

`npm run dev` starts Tauri and the web UI dev server.
It does not require `opensession-server`.

## Build

```bash
cd desktop
npm run build
```

Build flow:

1. `web` static bundle build (`../web/build`)
2. Tauri desktop bundle

## Notes

- UI components are reused from `@opensession/ui` via the existing `web` app.
- In desktop runtime, session/capability/auth reads use Tauri commands backed by local DB.
- Optional: set `OPENSESSION_LOCAL_DB_PATH` to point to a custom sqlite file path.

## Release

- Product version is synchronized from workspace `Cargo.toml` to desktop files via `scripts/sync-product-version.mjs`.
- Run `node scripts/sync-product-version.mjs --check` before release, or `--write` to apply.
- GitHub Actions `Release` workflow (manual) now runs:
  1. `release-plz update` + release publish
  2. macOS Tauri bundle build (`.dmg`, `.app.zip`, checksum)
  3. upload artifacts to tag `v<workspace-version>`
