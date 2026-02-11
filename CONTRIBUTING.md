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
   cargo clippy && cargo test
   ```
4. API route changes: update the endpoint table in README.

## Database Migrations

- Place new migration files in `migrations/`.
- Migrations are embedded via `include_str!` and run sequentially on startup.
- Test both creating a fresh DB and migrating from the previous version.

## Code Style

- Follow existing patterns in the codebase.
- Workspace clippy lints apply (see root `Cargo.toml`).

## License

By contributing, you agree that your contributions will be licensed under the MIT license.
