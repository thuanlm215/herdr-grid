# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-09-04

### Added

- Save the current preview as a reusable custom layout with a selected-pane
  anchor.
- Browse saved layouts alongside built-in presets and apply them to the current
  tab.
- Rename and delete custom layouts from the preset gallery.
- Persist geometry-only layouts in a bounded, versioned JSON catalog using
  atomic writes.
- Add mouse-first Editor and Presets tabs with distinct, contextual actions.
- Show add controls directly on the selected pane without a separate Add mode.
- Limit custom layouts to a single nine-card gallery.
- Show a contextual Delete action when a draft pane is selected.

### Changed

- Shorten the contextual footer and remove the redundant Reset toolbar button;
  the `r` shortcut remains available.

### Removed

- Remove the new-workspace preset destination to keep preset selection focused
  on the active tab.

## [0.3.1] - 2026-09-01

### Added

- Install a matching prebuilt binary on Linux and macOS, verify it against the
  release SHA-256 checksums, and fall back to a locked Cargo build when an
  asset is unavailable.

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

[Unreleased]: https://github.com/thuanlm215/herdr-grid/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/thuanlm215/herdr-grid/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/thuanlm215/herdr-grid/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/thuanlm215/herdr-grid/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/thuanlm215/herdr-grid/releases/tag/v0.1.0
