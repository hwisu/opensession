# Contributing to opensession

## Reporting Bugs

- Search existing issues first.
- **Reproduction steps are required** — issues without clear repro may be closed.
- Include: OS, Rust version, Docker version (if applicable), steps to reproduce, expected vs actual behavior, and relevant logs.

## Pull Requests

1. Fork → feature branch → PR to `main`.
2. One concern per PR.
3. Run before submitting:
   ```bash
   cargo clippy --workspace && cargo test --workspace
   ```
4. API route changes: update the endpoint table in README.

## Project Structure

Single Cargo workspace with 12 crates under `crates/`:

```
crates/
├── core          # HAIL domain model (pure types)
├── parsers       # Session file parsers
├── api           # Shared API types, SQL builders, service logic
├── api-client    # HTTP client
├── local-db      # Local SQLite database
├── git-native    # Git operations via gix
├── server        # Axum HTTP server
├── daemon        # Background file watcher and sync
├── cli           # CLI entry point (binary: opensession)
├── tui           # Terminal UI
├── worker        # Cloudflare Worker (excluded, wasm target)
└── e2e           # End-to-end tests
```

## Architectural Rules

- **`api` crate**: types + SQL builders + pure functions only. No HTTP/DB/framework code.
- **SQL queries**: centralize in `crates/api/src/db/`. No inline SQL in route handlers.
- **API responses**: always typed structs, never `serde_json::json!()`.
- **Feature naming**: backend feature is `backend` (not `server`).
- **Deployment profiles**:
  - Server/Axum supports auth and session upload/listing.
  - Worker/Wrangler is public read-only browsing.
  - Web UI profile is selected at build-time via `VITE_APP_PROFILE`.

## Database Migrations

- Two consolidated files: `crates/api/migrations/0001_schema.sql` (remote) and `local_0001_schema.sql` (local).
- Migrations are embedded via `include_str!` and run on startup.
- Test both creating a fresh DB and migrating from the previous version.

## Code Style

- Follow existing patterns in the codebase.
- Workspace clippy lints apply (see root `Cargo.toml`).

## License

By contributing, you agree that your contributions will be licensed under the MIT license.
