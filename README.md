# herdr-grid

[![CI](https://github.com/thuanlm215/herdr-grid/actions/workflows/ci.yml/badge.svg)](https://github.com/thuanlm215/herdr-grid/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/thuanlm215/herdr-grid)](https://github.com/thuanlm215/herdr-grid/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A visual layout editor for live [Herdr](https://herdr.dev/) panes. Drag to
rearrange, resize splits, add shells, and apply fixed presets without
restarting agents or losing pane state.

[![herdr-grid demo: rearrange panes, apply a preset, and add shells](docs/images/herdr-grid-demo.gif)](docs/images/herdr-grid-demo.webm)

## Quick start

```sh
herdr plugin install thuanlm215/herdr-grid
herdr plugin action invoke open --plugin herdr-grid
```

The editor opens over the active tab and performs no writes until you press
`Enter`. Existing PTYs, processes, scrollback, and pane identities are
preserved. Apply validates the live layout and attempts recovery if an
operation fails.

For one-key access, add the [`prefix + t` shortcut](#optional-shortcut-prefix--t)
after installation.

## Screenshots

### Arrange and resize panes

![Layout editor showing three live Herdr panes](docs/images/layout-editor.png)

### Add shell panes

Draft panes are highlighted in green and are created only when the completed
preview is applied.

![Add pane mode showing a green draft pane and edge controls](docs/images/add-pane-mode.png)

### Layout presets

Press `p` to open a visual gallery of fixed layouts, including 2×2 and 3×3
grids and asymmetric main-pane layouts. Apply the preset to the current tab or
use it to create a fresh workspace.

![Layout preset gallery with nine fixed pane arrangements](docs/images/layout-presets.png)

## Features

- Drag a pane onto another pane to swap their positions.
- Drop on an edge to create a new horizontal or vertical relationship.
- Drag split dividers to resize panes.
- Preview one or more new shell panes and create them together on Apply.
- Choose a fixed layout preset; missing slots become new shell panes.
- Build a preset in the current tab or in a newly created workspace.
- Balance every split in the preview to 50/50 with one key.
- Rearrange and resize the layout with keyboard controls.
- Undo or reset changes before they reach Herdr.
- Apply the preview explicitly with `Enter`; cancel safely with `Esc`.
- Preserve live PTYs, pane identities, process state, and scrollback.
- Revalidate the live layout before applying any change.
- Reconcile ambiguous API outcomes and attempt to restore the original layout
  after a partial failure.

## Requirements

- Herdr 0.8.2 or newer
- Linux or macOS
- A terminal with mouse-event support
- Either `curl`/`wget` plus `sha256sum`/`shasum`, or Rust stable and Cargo

Windows is not supported yet. Installation downloads a verified prebuilt
binary for ARM64 or AMD64 Linux and macOS. Rust stable and Cargo are required
only when no matching release is available or when building from source.

## Install

Install directly from GitHub with Herdr:

```sh
herdr plugin install thuanlm215/herdr-grid
```

Herdr clones the repository, shows the manifest for review, downloads the
matching release binary, verifies its SHA-256 checksum, and registers the
plugin. If the download is unavailable, installation falls back to a locked
Cargo build.

### Optional shortcut: `prefix + t`

Run once to bind `prefix + t` (usually `Ctrl+b`, then `t`):

```sh
herdr_grid_config="${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}"
mkdir -p "$(dirname "$herdr_grid_config")"
printf '\n[[keys.command]]\nkey = "prefix+t"\ntype = "plugin_action"\ncommand = "herdr-grid.open"\n' >> "$herdr_grid_config"
herdr config check && herdr server reload-config
```

## Usage

### Controls

| Input | Action |
| --- | --- |
| Drag pane to center | Swap two panes |
| Drag pane to edge | Re-parent pane at that edge |
| Drag divider | Resize a split |
| Click pane | Select pane |
| `n` | Enter Add pane mode |
| `p` | Open the fixed layout preset gallery |
| Click pane, then edge `+` | Add a draft shell at that edge |
| `d` in Add pane mode | Remove the selected draft pane |
| `Enter` in Add pane mode | Keep drafts in the preview and return to normal mode |
| `Esc` in Add pane mode | Discard the complete preview and return to normal mode |
| Arrow keys or `h/j/k/l` | Move selection |
| `Space` | Pick up or drop selected pane |
| `[` / `]` | Resize selected split |
| `u` | Undo last preview edit |
| `r` | Restore the initial preview |
| `=` in the normal mode | Balance every split in the preview to 50/50 |
| `Enter` | Validate and apply preview |
| `Esc` or `q` in the normal mode | Cancel without applying |
| `?` | Open the complete in-app help |

For a read-only connectivity check from inside a Herdr-managed pane:

```sh
target/release/herdr-grid --inspect
```

## Safety model

`herdr-grid` edits a local layout model while the popup is open. Apply then:

1. Acquires a process-wide apply lock.
2. Verifies the workspace, tab, pane membership, topology, and split ratios
   against the opening snapshot. Pane output and agent-status revisions do not
   invalidate the preview.
3. Creates requested shell panes, then builds the complete operation plan.
4. Checks the authoritative Herdr layout after each operation.
5. Re-discovers pane locations and rebuilds the original layout if an API
   response is lost or an operation fails.

Structural edits temporarily park non-anchor panes in labelled scratch tabs,
then rebuild the requested tree with `pane.move`. Empty scratch tabs are
removed automatically by Herdr.

The plugin never closes a pane that existed when the editor opened. If Apply
fails after creating a requested shell, it may close only that newly created
pane while restoring the original layout. It never calls `layout.apply` or
`tab.close`. See [Architecture](docs/architecture.md) for details and remaining
failure modes.

For a new-workspace preset, Apply creates an isolated workspace first and
constructs its split tree from Herdr's returned pane IDs. If construction or
verification fails, the plugin closes only that newly created workspace. The
source workspace is never rearranged.

## Development

### Local development

Build and link a local checkout:

```sh
git clone https://github.com/thuanlm215/herdr-grid.git
cd herdr-grid
cargo build --release --locked
herdr plugin link . --enabled
```

If it was linked in disabled mode, run `herdr plugin enable herdr-grid`.

### Quality checks

Run the complete local quality gate:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release --locked
```

Live mutation tests must use a named disposable Herdr session. Do not run
structural experiments against the default session.

See [CONTRIBUTING.md](CONTRIBUTING.md) before submitting a change.
Notable user-facing changes are recorded in [CHANGELOG.md](CHANGELOG.md).

## License

Licensed under the [MIT License](LICENSE).
