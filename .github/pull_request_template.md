## Summary

Describe the user-visible change and why it is needed.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [ ] `cargo test --locked`
- [ ] Live mutation testing, if any, used a disposable Herdr session.

## Safety

Explain any effect on live panes, validation, rollback, or recovery. Write
`None` when the change cannot mutate a Herdr session.
