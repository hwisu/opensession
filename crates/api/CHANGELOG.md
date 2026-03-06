# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.34](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.33...opensession-api-v0.2.34) - 2026-03-06

### Added

- add summary batch progress/scope and lifecycle TTL controls
- split local/app setup and add quick-share flow with tests
- *(desktop)* add optional change reader with session Q&A
- migrate desktop vector search to chunk-indexed bge-m3 workflow
- *(review)* expose Q&A digest content and sync validation docs

### Other

- Improve session/runtime UX and explicit storage migration
- remove custom coverage criteria gates
- replace happy-path matrix with domain function coverage gate
- redesign app+cli summary pipeline and remove tui

## [0.2.33](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.32...opensession-api-v0.2.33) - 2026-03-03

### Added

- *(desktop)* add tauri handoff build flow in session detail
- unify desktop/web contracts and stabilize session timeline UX

## [0.2.28](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.27...opensession-api-v0.2.28) - 2026-02-26

### Added

- add gitlab/raw-git review flow with credentialed worker access

### Other

- Harden git source auth and web session security

## [0.2.27](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.26...opensession-api-v0.2.27) - 2026-02-26

### Other

- Add local PR review workflow via ops review

## [0.2.26](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.25...opensession-api-v0.2.26) - 2026-02-23

### Other

- *(schema)* remove legacy users api_key/avatar columns

## [0.2.25](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.24...opensession-api-v0.2.25) - 2026-02-23

### Other

- remove legacy compat and flatten migrations

## [0.2.24](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.23...opensession-api-v0.2.24) - 2026-02-23

### Other

- codex/git native v2 hidden ref cleanup ([#9](https://github.com/hwisu/opensession/pull/9))

## [0.2.23](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.22...opensession-api-v0.2.23) - 2026-02-22

### Other

- DX reset v1: source URI + local-first register/share/handoff ([#7](https://github.com/hwisu/opensession/pull/7))

## [0.2.22](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.21...opensession-api-v0.2.22) - 2026-02-21

### Other

- Refactor landing/docs, remove upload UI, and add git preview flow

## [0.2.20](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.19...opensession-api-v0.2.20) - 2026-02-20

### Other

- centralize session role handling and local-db auxiliary filtering

## [0.2.17](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.16...opensession-api-v0.2.17) - 2026-02-20

### Other

- remove deprecated summary and team remnants

## [0.2.16](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.15...opensession-api-v0.2.16) - 2026-02-19

### Added

- add GitHub URL ingest preview session renderer

## [0.2.15](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.14...opensession-api-v0.2.15) - 2026-02-19

### Other

- drop legacy paths and align tui/session pipelines

## [0.2.11](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.10...opensession-api-v0.2.11) - 2026-02-19

### Other

- test pushpush

## [0.2.10](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.9...opensession-api-v0.2.10) - 2026-02-19

### Other

- Prune team surfaces and complete git-native runtime cleanup

## [0.2.9](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.8...opensession-api-v0.2.9) - 2026-02-19

### Other

- prune docker/team surfaces and align on git-native public sessions

## [0.2.7](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.6...opensession-api-v0.2.7) - 2026-02-16

### Added

- *(worker)* guest landing flow, oauth diagnostics, and config cleanup

## [0.2.6](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.5...opensession-api-v0.2.6) - 2026-02-16

### Fixed

- *(oauth)* trim provider credentials and prefer configured base url

## [0.2.5](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.4...opensession-api-v0.2.5) - 2026-02-16

### Added

- unify parser/ui behavior and fix oauth/public-feed flow

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-api-v0.2.0...opensession-api-v0.2.1) - 2026-02-16

### Added

- *(storage)* use sqlite summary cache and default local git-native
- improve live/session UX, parity updates, and infra fixes

### Other

- stage all pending changes

## [0.1.3](https://github.com/hwisu/opensession/compare/opensession-api-v0.1.2...opensession-api-v0.1.3) - 2026-02-13

### Other

- git-native crate, shadow commands, CLI enhancements, DB schema updates
- unify DB schema with sea-query and dev context columns

## [0.1.2](https://github.com/hwisu/opensession/compare/opensession-api-v0.1.1...opensession-api-v0.1.2) - 2026-02-12

### Added

- *(tui)* full overhaul — tab navigation, teams, invitations, settings
