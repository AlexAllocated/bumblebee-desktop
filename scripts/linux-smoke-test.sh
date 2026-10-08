#!/usr/bin/env bash
set -euo pipefail
# CI creates an isolated, empty keyring. Never run this against an existing user's keyring.
test -n "${RUNNER_TEMP:-}" || { echo 'This helper is for isolated CI runners.' >&2; exit 1; }
smoke_root=$(mktemp -d "$RUNNER_TEMP/bumblebee-smoke.XXXXXXXX")
export XDG_DATA_HOME="$smoke_root/data"
export XDG_CONFIG_HOME="$smoke_root/config"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
eval "$(gnome-keyring-daemon --start --components=secrets)"
printf '%s' 'temporary-ci-keyring' | gnome-keyring-daemon --unlock
if [ "$#" -eq 0 ]; then
  set -- bumblebee-desktop
fi
timeout 120s "$@" --smoke-test
