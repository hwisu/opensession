# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/hwisu/opensession/compare/opensession-v0.1.4...opensession-v0.2.0) - 2026-02-15

### Added

- improve session loading, filtering, and multi-column UX parity
- upgrade view timeline summary pipeline and parser lane signals
- unify IA and timeline summary across CLI TUI Web

### Fixed

- add missing fields to UserSettingsResponse in me()

### Other

- *(db)* enforce migration parity across server worker local
- Fix timeline summary generation and CI format drift
- Fix clippy regressions in tui and cli
- Improve web/docker/tui parity and unify session presentation
- DRY command/settings flow and add docker playwright full-test
- extract shared config to core, fix inline SQL, overhaul docs
- consolidate three repos into monorepo
- Remove duplicate crates, use shared ServiceError and service module
- Add CI workflow, pre-commit hook, migration, and apply cargo fmt
- Implement daemon enhancement plan: stubs, config sync, realtime streaming
- Add handoff, server commands, retry logic, TUI server status
- Fix TS errors, convert EventView to $derived, update clippy lints, fix upload page
- Fix CLI/daemon for team model: add team_id config, API key auth, remove legacy fallback
- Initial commit

## [0.1.3](https://github.com/hwisu/opensession-core/compare/opensession-v0.1.2...opensession-v0.1.3) - 2026-02-12

### Other

- update Cargo.lock dependencies
