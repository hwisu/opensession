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
