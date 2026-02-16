# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-local-db-v0.2.0...opensession-local-db-v0.2.1) - 2026-02-16

### Added

- *(storage)* use sqlite summary cache and default local git-native
- improve live/session UX, parity updates, and infra fixes

### Other

- Fix tui workspace run, summary pipeline, and parity gates
- stage all pending changes

## [0.2.0](https://github.com/hwisu/opensession/compare/opensession-local-db-v0.1.4...opensession-local-db-v0.2.0) - 2026-02-15

### Added

- improve session loading, filtering, and multi-column UX parity
- unify IA and timeline summary across CLI TUI Web

### Other

- *(db)* enforce migration parity across server worker local
- Improve web/docker/tui parity and unify session presentation
- consolidate three repos into monorepo
- Remove duplicate crates, use shared ServiceError and service module
- Remove cli, tui, local-db from workspace; point to opensession-core

## [0.1.3](https://github.com/hwisu/opensession/compare/opensession-local-db-v0.1.2...opensession-local-db-v0.1.3) - 2026-02-13

### Fixed

- cargo fmt and ci audit cache collision

### Other

- git-native crate, shadow commands, CLI enhancements, DB schema updates
- unify DB schema with sea-query and dev context columns
- remove stub Goose/Aider parsers and dead code
