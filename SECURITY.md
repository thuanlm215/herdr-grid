# Security policy

## Supported versions

`herdr-grid` has not reached its first stable release. Security fixes are made
on the latest `main` branch only.

## Reporting a vulnerability

Please do not open a public issue for vulnerabilities involving command
execution, socket access, path handling, terminal escape sequences, or loss of
live pane state.

Use GitHub's private vulnerability reporting feature from the repository's
Security tab. Include:

- the affected revision;
- impact and reproduction steps;
- whether the issue requires untrusted workspace contents or pane output;
- any suggested mitigation.

Do not include credentials, private pane output, or destructive proof-of-
concept commands. Reports will be acknowledged as soon as practical; no fixed
response SLA is offered before the first stable release.
