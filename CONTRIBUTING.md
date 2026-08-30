# Contributing to herdr-grid

Thanks for helping improve `herdr-grid`.

## Before opening an issue

- Search existing issues for the same behavior.
- Confirm the problem with the latest `main` branch.
- Include your Herdr version, operating system, terminal, and the smallest
  layout that reproduces the problem.
- Remove pane output, paths, or process details that may contain secrets.

For security-sensitive reports, follow [SECURITY.md](SECURITY.md) instead of
opening a public issue.

## Development setup

You need Herdr 0.8.2 or newer and a stable Rust toolchain.

```sh
cd herdr-grid
cargo build --locked
cargo test
```

Link a development build only when you need to exercise the plugin surface:

```sh
cargo build --release --locked
herdr plugin link . --disabled
```

Enable it explicitly after you have reviewed the target Herdr session.

## Pull requests

Keep changes focused and include tests for observable behavior. Before opening
a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
```

Changes to the transaction engine should include failure-injection coverage.
Any live mutation experiment must run in a named disposable Herdr session,
never the default session.

## Safety invariants

Contributions must preserve these rules:

- Never use `layout.apply` for a live tab.
- Never intentionally close or terminate a user's pane.
- Never mutate Herdr while the user is still editing the preview.
- Revalidate live state immediately before Apply.
- Preserve every pane and report its last known location after a failed
  recovery.

Read [docs/architecture.md](docs/architecture.md) before changing planning,
execution, reconciliation, or rollback code.
