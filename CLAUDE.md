# opensession.io

## Build & Lint

After completing any code changes, always run these checks before presenting results:

```bash
# Rust: check + clippy (workspace)
cargo clippy --workspace 2>&1 | grep -E "^error"

# Frontend: build check
cd web && npx vite build 2>&1 | tail -3

# Tests
cargo test --workspace
```

## Project Structure

- Rust workspace: 8 crates (core, parsers, cli, daemon, server, worker, tui, api-types)
- Frontend: SvelteKit in `web/`
- Format: HAIL (Human AI Interaction Log), version "hail-1.0.0"
- Worker crate is NOT in the workspace (builds with wasm target separately)

## Key Commands

- `cargo test --workspace` — run all tests
- `cargo test -p opensession-api-types -- export_typescript` — regenerate TS types
- `cd web && npx vite build` — build frontend
- `cargo run -p opensession-cli -- discover` — discover local sessions
