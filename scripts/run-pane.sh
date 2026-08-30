#!/usr/bin/env bash
set -uo pipefail

plugin_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
log_file="${HERDR_PLUGIN_CONFIG_DIR:-${TMPDIR:-/tmp}}/herdr-grid-pane.log"

mkdir -p "$(dirname "$log_file")"
{
  printf 'started=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'herdr_env=%s pane=%s active_pane=%s context=%s\n' \
    "${HERDR_ENV:-unset}" \
    "${HERDR_PANE_ID:-unset}" \
    "${HERDR_ACTIVE_PANE_ID:-unset}" \
    "$([[ -n "${HERDR_PLUGIN_CONTEXT_JSON:-}" ]] && printf present || printf absent)"
} >"$log_file"

set +e
"$plugin_root/target/release/herdr-grid" \
  2> >(tee -a "$log_file" >&2)
status=$?
set -e

if (( status != 0 )); then
  printf '\nherdr-grid failed to start (exit %d).\n' "$status" | tee -a "$log_file" >&2
  printf 'Diagnostic log: %s\n' "$log_file" | tee -a "$log_file" >&2
  printf 'Press Enter to close this popup.\n' >&2
  read -r _ || true
fi

exit "$status"
