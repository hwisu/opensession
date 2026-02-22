# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.23](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.22...opensession-core-v0.2.23) - 2026-02-22

### Other

- DX reset v1: source URI + local-first register/share/handoff ([#7](https://github.com/hwisu/opensession/pull/7))

## [0.2.21](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.20...opensession-core-v0.2.21) - 2026-02-20

### Other

- Improve session and handoff UX, tracking, and live-state accuracy

## [0.2.20](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.19...opensession-core-v0.2.20) - 2026-02-20

### Other

- centralize session role handling and local-db auxiliary filtering

## [0.2.18](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.17...opensession-core-v0.2.18) - 2026-02-20

### Added

- *(handoff)* switch to git-ref artifacts as source of truth

## [0.2.17](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.16...opensession-core-v0.2.17) - 2026-02-20

### Other

- remove deprecated summary and team remnants

## [0.2.14](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.13...opensession-core-v0.2.14) - 2026-02-19

### Other

- Improve handoff temporal consistency and execution timeline

## [0.2.13](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.12...opensession-core-v0.2.13) - 2026-02-19

### Other

- Simplify handoff flow and add provider populate mode

## [0.2.12](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.11...opensession-core-v0.2.12) - 2026-02-19

### Other

- Remove legacy handoff paths and clean publish/docs

## [0.2.9](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.8...opensession-core-v0.2.9) - 2026-02-19

### Other

- prune docker/team surfaces and align on git-native public sessions

## [0.2.4](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.3...opensession-core-v0.2.4) - 2026-02-16

### Other

- *(boundaries)* centralize parser attribute contracts

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-core-v0.2.0...opensession-core-v0.2.1) - 2026-02-16

### Added

- improve live/session UX, parity updates, and infra fixes

### Fixed

- *(core)* keep semver-stable config API

### Other

- *(handoff)* enrich objective and task summary signal
- stage all pending changes

## [0.2.0](https://github.com/hwisu/opensession/compare/opensession-core-v0.1.4...opensession-core-v0.2.0) - 2026-02-15

### Added

- upgrade view timeline summary pipeline and parser lane signals
- unify IA and timeline summary across CLI TUI Web

### Other

- Improve web/docker/tui parity and unify session presentation
- extract shared config to core, fix inline SQL, overhaul docs

## [0.1.3](https://github.com/hwisu/opensession/compare/opensession-core-v0.1.2...opensession-core-v0.1.3) - 2026-02-13

### Fixed

- cargo fmt and ci audit cache collision

### Other

- git-native crate, shadow commands, CLI enhancements, DB schema updates
- unify DB schema with sea-query and dev context columns
