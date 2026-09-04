# Architecture

This document describes the public safety and execution model of
`herdr-grid`. Internal implementation plans are intentionally not part of the
repository.

## Components

The codebase is divided into four layers:

- `model`: pure live and reusable binary split trees plus safe edit operations;
- `ui`: Ratatui rendering, geometry, hit testing, mouse input, and keyboard
  input;
- `herdr`: protocol parsing, operation planning, transaction execution, and
  recovery;
- `app`: preview state, selection, undo, reset, and Apply coordination.
- `saved`: versioned custom-layout validation and atomic persistence.

The model has no Herdr or terminal dependency. This keeps preview edits and
planner tests deterministic.

## Layout representation

A tab is represented as a binary tree. Leaves retain public pane IDs; internal
nodes contain a horizontal or vertical direction and a bounded ratio.

Herdr's `splits` metadata is authoritative for topology, direction, and ratio.
Rectangle reconstruction exists only as a compatibility fallback when split
metadata is absent.

## Apply paths

Shape-preserving edits use the shortest available operations:

- `pane.swap` changes leaf positions;
- `layout.set_split_ratio` changes an internal split ratio.

Shape-changing edits use a stable-anchor transaction:

1. Keep one pane in the original tab as the stable anchor.
2. Move every other pane into its own labelled scratch tab.
3. Rebuild the target tree around the anchor with ordered `pane.move` calls.
4. Swap pane identities into their requested leaves when necessary.
5. Apply final ratios and verify the resulting tree.

All scratch pane and tab IDs come from Herdr responses. The executor does not
predict identifiers.

New panes use reserved draft IDs only inside the preview model. On Apply, the
executor creates each shell with `pane.split`, replaces draft IDs with the
authoritative IDs returned by Herdr, and then runs the normal layout planner.
Deleting a draft before Apply is therefore a model-only edit. If a later Apply
step fails, recovery restores the expanded intermediate layout before closing
only the panes created by that Apply attempt.

Fixed presets are pure model constructors. For the current tab, existing pane
IDs fill the preset slots and any missing slots receive draft IDs. A preset
with fewer slots than the current preview is rejected rather than deleting a
pane. For main-pane variants, the selected pane is assigned to the main slot.

Saved layouts use a separate tree whose leaves are numbered slots rather than
Herdr pane IDs. Capture assigns slot numbers in visual reading order and stores
the selected pane's slot as the anchor. Reuse assigns the currently selected
pane to that anchor, maps the remaining panes in visual order, and fills empty
slots with drafts. A layout with fewer slots than existing panes is rejected;
saved layouts never imply deletion.

The global custom-layout catalog is a bounded, versioned JSON document under
`HERDR_PLUGIN_CONFIG_DIR`. It contains geometry and display names only. Loads
validate schema version, names, tree depth, ratios, slot uniqueness, and
catalog size. Writes use a temporary file in the same directory followed by an
atomic rename. A malformed or unknown-schema catalog disables writes for that
run so the original file cannot be overwritten.

## Validation and reconciliation

Before the first write, the transaction compares the live workspace, tab,
pane membership, topology, and ratios with the snapshot captured when the
editor opened. Generic pane revisions are intentionally excluded because
normal terminal output and agent-status updates advance them without changing
the layout.

Every write is followed by a layout check. Socket connect, write, and response
waits have bounded deadlines. A timeout or malformed response is treated as an
ambiguous outcome because the server may already have committed the mutation.

After an ambiguous result, the client inventories the workspace again,
resolves every pane's current tab, reads the layout through the stable anchor,
and rebuilds the original snapshot. If recovery also fails, the editor stops
issuing writes and reports the last authoritative pane and tab IDs available.

## Explicit non-goals

The transaction engine never:

- calls `layout.apply`, because it replaces live PTYs;
- closes a pane that existed before Apply or sends input to its process;
- closes a scratch tab directly;
- edits more than one source tab;
- moves panes between workspaces.
- deletes existing panes to make them fit a preset.

## Remaining limitations

- Staleness is enforced at Apply time but is not yet displayed continuously
  while the editor remains open.
- A second failure during recovery can leave panes distributed across labelled
  scratch tabs. The error reports their last known locations for manual action.
- Release qualification still needs a broader terminal and live-session test
  matrix.
