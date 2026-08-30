# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-08-30

### Added

- Balance every layout split to 50/50 from normal mode with `=`.
- Choose from fixed grid and main-pane layout presets with `p`, automatically
  filling missing slots with new shell panes.
- Create a preset in a fresh workspace without changing the source workspace.

## [0.2.1] - 2026-08-30

### Fixed

- Balance and precisely center the Add pane edge controls across terminal cell
  aspect ratios.

## [0.2.0] - 2026-08-30

### Added

- Add pane mode (`n`) with clickable edge targets, removable draft shells,
  and deferred shell creation on Apply.

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

[Unreleased]: https://github.com/thuanlm215/herdr-grid/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/thuanlm215/herdr-grid/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/thuanlm215/herdr-grid/releases/tag/v0.1.0
