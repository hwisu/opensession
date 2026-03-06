# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.34](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.33...opensession-runtime-config-v0.2.34) - 2026-03-06

### Added

- add summary batch progress/scope and lifecycle TTL controls
- *(desktop)* add optional change reader with session Q&A
- migrate desktop vector search to chunk-indexed bge-m3 workflow

### Other

- Improve session/runtime UX and explicit storage migration
- redesign app+cli summary pipeline and remove tui

## [0.2.25](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.24...opensession-runtime-config-v0.2.25) - 2026-02-23

### Other

- remove legacy compat and flatten migrations

## [0.2.23](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.22...opensession-runtime-config-v0.2.23) - 2026-02-22

### Other

- DX reset v1: source URI + local-first register/share/handoff ([#7](https://github.com/hwisu/opensession/pull/7))

## [0.2.17](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.16...opensession-runtime-config-v0.2.17) - 2026-02-20

### Other

- remove deprecated summary and team remnants

## [0.2.15](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.14...opensession-runtime-config-v0.2.15) - 2026-02-19

### Other

- drop legacy paths and align tui/session pipelines

## [0.2.11](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.10...opensession-runtime-config-v0.2.11) - 2026-02-19

### Other

- test pushpush

## [0.2.10](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.9...opensession-runtime-config-v0.2.10) - 2026-02-19

### Other

- Prune team surfaces and complete git-native runtime cleanup

## [0.2.8](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.7...opensession-runtime-config-v0.2.8) - 2026-02-19

### Added

- *(retention)* prune git-native session history on schedule

## [0.2.2](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.1...opensession-runtime-config-v0.2.2) - 2026-02-16

### Other

- apply staged docs and runtime updates

## [0.2.1](https://github.com/hwisu/opensession/compare/opensession-runtime-config-v0.2.0...opensession-runtime-config-v0.2.1) - 2026-02-16

### Added

- linearize timeline and automate release-plz update push

### Other

- Fix tui workspace run, summary pipeline, and parity gates
- *(tui)* simplify capture settings and add profile e2e coverage
- simplify tui flow and centralize config fallbacks
- stage all pending changes
