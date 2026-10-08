#!/usr/bin/env bash
set -euo pipefail
# CI creates an isolated, empty keyring. Never run this against an existing user's keyring.
test -n "${RUNNER_TEMP:-}" || { echo 'This helper is for isolated CI runners.' >&2; exit 1; }
export XDG_DATA_HOME="$RUNNER_TEMP/bumblebee-smoke-data"
export XDG_CONFIG_HOME="$RUNNER_TEMP/bumblebee-smoke-config"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
eval "$(gnome-keyring-daemon --start --components=secrets)"
printf '%s' 'temporary-ci-keyring' | gnome-keyring-daemon --unlock
bumblebee-desktop --smoke-test
