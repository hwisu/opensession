# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.21](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.20...opensession-parsers-v0.2.21) - 2026-02-20

### Other

- Improve session and handoff UX, tracking, and live-state accuracy

## [0.2.20](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.19...opensession-parsers-v0.2.20) - 2026-02-20

### Other

- centralize session role handling and local-db auxiliary filtering

## [0.2.19](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.18...opensession-parsers-v0.2.19) - 2026-02-20

### Other

- Improve TUI detail clarity, handoff UX, and live detection

## [0.2.18](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.17...opensession-parsers-v0.2.18) - 2026-02-20

### Added

- *(handoff)* switch to git-ref artifacts as source of truth

## [0.2.16](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.15...opensession-parsers-v0.2.16) - 2026-02-19

### Added

- add GitHub URL ingest preview session renderer

## [0.2.15](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.14...opensession-parsers-v0.2.15) - 2026-02-19

### Other

- drop legacy paths and align tui/session pipelines

## [0.2.4](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.3...opensession-parsers-v0.2.4) - 2026-02-16

### Added

- *(parsers)* unify five-tool parsing and timeline parity

### Other

- *(boundaries)* centralize parser attribute contracts

## [0.2.3](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.2...opensession-parsers-v0.2.3) - 2026-02-16

### Added

- *(tui)* improve session detail signal and density

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.2.0...opensession-parsers-v0.2.1) - 2026-02-16

### Added

- improve live/session UX, parity updates, and infra fixes

### Fixed

- *(cli)* make default scope mode compile-clean and clippy-clean

### Other

- fix claude subagent message_count expectation

## [0.2.0](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.1.4...opensession-parsers-v0.2.0) - 2026-02-15

### Added

- improve session loading, filtering, and multi-column UX parity
- upgrade view timeline summary pipeline and parser lane signals
- unify IA and timeline summary across CLI TUI Web

## [0.1.3](https://github.com/hwisu/opensession/compare/opensession-parsers-v0.1.2...opensession-parsers-v0.1.3) - 2026-02-13

### Other

- git-native crate, shadow commands, CLI enhancements, DB schema updates
- unify DB schema with sea-query and dev context columns
- remove stub Goose/Aider parsers and dead code
