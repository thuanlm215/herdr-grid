# herdr-grid

[![CI](https://github.com/thuanlm215/herdr-grid/actions/workflows/ci.yml/badge.svg)](https://github.com/thuanlm215/herdr-grid/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A visual drag-and-drop layout editor for live [Herdr](https://herdr.dev/)
panes.

`herdr-grid` opens as a popup for the active tab. Rearrange panes, resize
splits, preview the result, and apply the complete layout without restarting
the processes running inside those panes.

> [!IMPORTANT]
> This project is under active development. Test it with a disposable Herdr
> session before using it with important long-running processes.

## Features

- Drag a pane onto another pane to swap their positions.
- Drop on an edge to create a new horizontal or vertical relationship.
- Drag split dividers to resize panes.
- Use the same editor entirely from the keyboard.
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
- Rust stable and Cargo when building from source

Windows is not supported yet. Tagged releases provide prebuilt binaries for
ARM64 and AMD64 macOS, plus statically linked ARM64 and AMD64 Linux.

## Install

Install directly from GitHub with Herdr:

```sh
herdr plugin install thuanlm215/herdr-grid
```

Herdr clones the repository, shows the manifest for review, builds the release
binary, and registers the plugin.

### Local development

From a local checkout, build the release binary and link the plugin:

```sh
git clone https://github.com/thuanlm215/herdr-grid.git
cd herdr-grid
cargo build --release --locked
herdr plugin link . --enabled
```

If the plugin was previously linked in disabled mode, enable it with:

```sh
herdr plugin enable herdr-grid
```

## Usage

Open the editor through its plugin action:

```sh
herdr plugin action invoke open --plugin herdr-grid
```

The editor reads the tab underneath the popup and performs no writes until
you apply the preview.

### Optional shortcut: `prefix + t`

Run the following block to bind `prefix + t` to the layout editor. It appends
the binding without overwriting the rest of your Herdr configuration, avoids
adding it twice, and stops if another command already uses the same key:

```sh
herdr_grid_config="${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}"
mkdir -p "$(dirname "$herdr_grid_config")"
touch "$herdr_grid_config"

if grep -Fq 'command = "herdr-grid.open"' "$herdr_grid_config"; then
  echo "herdr-grid shortcut is already configured"
elif grep -Eq '^[[:space:]]*key[[:space:]]*=[[:space:]]*"prefix\+t"' "$herdr_grid_config"; then
  echo "prefix+t is already assigned; edit $herdr_grid_config manually" >&2
  exit 1
else
  tee -a "$herdr_grid_config" >/dev/null <<'EOF'

[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-grid.open"
EOF
  herdr config check
  herdr server reload-config
fi
```

Herdr's default prefix is `Ctrl+b`, so press `Ctrl+b`, release it, then press
`t`. If you configured a different prefix, use that key followed by `t`.

### Controls

| Input | Action |
| --- | --- |
| Drag pane to center | Swap two panes |
| Drag pane to edge | Re-parent pane at that edge |
| Drag divider | Resize a split |
| Click pane | Select pane |
| Arrow keys or `h/j/k/l` | Move selection |
| `Space` | Pick up or drop selected pane |
| `[` / `]` | Resize selected split |
| `u` | Undo last preview edit |
| `r` | Restore the initial preview |
| `Enter` | Validate and apply preview |
| `Esc` or `q` | Cancel without applying |
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
3. Builds the complete operation plan before executing it.
4. Checks the authoritative Herdr layout after each operation.
5. Re-discovers pane locations and rebuilds the original layout if an API
   response is lost or an operation fails.

Structural edits temporarily park non-anchor panes in labelled scratch tabs,
then rebuild the requested tree with `pane.move`. Empty scratch tabs are
removed automatically by Herdr.

The plugin never calls `layout.apply`, `pane.close`, or `tab.close`. See
[Architecture](docs/architecture.md) for details and remaining failure modes.

## Development

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

## Project status

The core editor, direct swap/resize path, structural transaction engine, and
failure reconciliation are implemented. Work remaining before the first
stable release includes:

- continuous stale-state indication while the popup remains open;
- broader terminal and live-session compatibility testing;
- release screenshots and a short demonstration recording.

## License

Licensed under the [MIT License](LICENSE).
