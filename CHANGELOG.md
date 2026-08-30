# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-08-30

### Added

- Native ARM64 Linux release artifact using the statically linked
  `aarch64-unknown-linux-musl` target.

### Fixed

- Allow layout changes to be applied while panes produce output or agents
  transition between working and idle, while still rejecting real layout
  drift.

## [0.1.0] - 2026-08-30

### Added

- Popup editor that mirrors the active Herdr tab's pane layout.
- Mouse and keyboard workflows for swapping, re-parenting, and resizing panes.
- Undo, reset, explicit apply, and cancel-before-write behavior.
- Pre-apply stale-state validation and post-operation verification.
- Transaction recovery for ambiguous or partially completed Herdr operations.

[Unreleased]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/thuanlm215/herdr-grid/releases/tag/v0.1.0
