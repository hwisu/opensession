# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.14](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.14) - 2026-02-19

### Other

- Improve handoff temporal consistency and execution timeline
- *(release)* apply release-plz update
- Simplify handoff flow and add provider populate mode
- *(release)* apply release-plz update
- Remove legacy handoff paths and clean publish/docs
- *(release)* apply release-plz update
- test pushpush
- *(release)* apply release-plz update
- Fix TUI env-flag test flake and docs
- Prune team surfaces and complete git-native runtime cleanup
- *(release)* apply release-plz update
- prune docker/team surfaces and align on git-native public sessions

## [0.2.13](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.13) - 2026-02-19

### Other

- Simplify handoff flow and add provider populate mode
- *(release)* apply release-plz update
- Remove legacy handoff paths and clean publish/docs
- *(release)* apply release-plz update
- test pushpush
- *(release)* apply release-plz update
- Fix TUI env-flag test flake and docs
- Prune team surfaces and complete git-native runtime cleanup
- *(release)* apply release-plz update
- prune docker/team surfaces and align on git-native public sessions

## [0.2.12](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.12) - 2026-02-19

### Other

- Remove legacy handoff paths and clean publish/docs
- *(release)* apply release-plz update
- test pushpush
- *(release)* apply release-plz update
- Fix TUI env-flag test flake and docs
- Prune team surfaces and complete git-native runtime cleanup
- *(release)* apply release-plz update
- prune docker/team surfaces and align on git-native public sessions

## [0.2.11](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.11) - 2026-02-19

### Other

- test pushpush
- *(release)* apply release-plz update
- Fix TUI env-flag test flake and docs
- Prune team surfaces and complete git-native runtime cleanup
- *(release)* apply release-plz update
- prune docker/team surfaces and align on git-native public sessions

## [0.2.10](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.10) - 2026-02-19

### Other

- Fix TUI env-flag test flake and docs
- Prune team surfaces and complete git-native runtime cleanup
- *(release)* apply release-plz update
- prune docker/team surfaces and align on git-native public sessions

## [0.2.9](https://github.com/hwisu/opensession/compare/opensession-v0.2.8...opensession-v0.2.9) - 2026-02-19

### Other

- prune docker/team surfaces and align on git-native public sessions

## [0.2.8](https://github.com/hwisu/opensession/compare/opensession-v0.2.7...opensession-v0.2.8) - 2026-02-19

### Added

- *(retention)* prune git-native session history on schedule

## [0.2.7](https://github.com/hwisu/opensession/compare/opensession-v0.2.6...opensession-v0.2.7) - 2026-02-16

### Added

- *(worker)* guest landing flow, oauth diagnostics, and config cleanup

## [0.2.6](https://github.com/hwisu/opensession/compare/opensession-v0.2.5...opensession-v0.2.6) - 2026-02-16

### Fixed

- *(worker)* honor public feed flag for session listing

## [0.2.5](https://github.com/hwisu/opensession/compare/opensession-v0.2.4...opensession-v0.2.5) - 2026-02-16

### Added

- unify parser/ui behavior and fix oauth/public-feed flow

## [0.2.4](https://github.com/hwisu/opensession/compare/opensession-v0.2.3...opensession-v0.2.4) - 2026-02-16

### Other

- *(boundaries)* centralize parser attribute contracts

## [0.2.2](https://github.com/hwisu/opensession/compare/opensession-v0.2.1...opensession-v0.2.2) - 2026-02-16

### Other

- apply staged docs and runtime updates

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-v0.2.0...opensession-v0.2.1) - 2026-02-16

### Added

- *(storage)* use sqlite summary cache and default local git-native
- prefer BASE_URL for server public URL

### Fixed

- *(cli)* make default scope mode compile-clean and clippy-clean
- fix oauth redirect base URL to prevent github callback mismatch

### Other

- Fix tui workspace run, summary pipeline, and parity gates
- *(tui)* simplify capture settings and add profile e2e coverage
- simplify tui flow and centralize config fallbacks
- *(cli)* make handoff pipe-friendly defaults for nu
- *(handoff)* enrich objective and task summary signal
- stage all pending changes

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

## [0.1.3](https://github.com/hwisu/opensession/compare/opensession-v0.1.2...opensession-v0.1.3) - 2026-02-12

### Other

- update Cargo.lock dependencies
