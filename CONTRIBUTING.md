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
   ./.githooks/pre-commit
   ./.githooks/pre-push
   ```
4. For web/runtime-web changes, validate at least one real user path with `wrangler dev` + Playwright live suite.
5. API route changes: update the endpoint table in README.

Validation details (local + CI parity) live in `docs/development-validation-flow.md`.

## Project Structure

Single Cargo workspace with 13 crates under `crates/`:

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
├── runtime-config # Runtime settings config and validation
├── summary       # Summary extraction/normalization domain
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
  - Worker/Wrangler serves public browsing with runtime-gated auth.
  - Web UI behavior is selected at runtime via `GET /api/capabilities`.

## Database Migrations

- Two consolidated files: `crates/api/migrations/0001_schema.sql` (remote) and `local_0001_schema.sql` (local).
- Migrations are embedded via `include_str!` and run on startup.
- Test both creating a fresh DB and migrating from the previous version.

## Code Style

- Follow existing patterns in the codebase.
- Rust code targets Edition 2024 and the repo pins Rust 1.93.0 via `mise.toml`.
- New workspace crates should inherit `edition.workspace = true` and `rust-version.workspace = true`.
- Keep `unsafe` in the smallest possible helper and include a `SAFETY:` comment for each block.
- Workspace clippy lints apply (see root `Cargo.toml`).

## License

By contributing, you agree that your contributions will be licensed under the MIT license.
