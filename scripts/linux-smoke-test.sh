#!/usr/bin/env bash
set -euo pipefail
# CI creates an isolated, empty keyring. Never run this against an existing user's keyring.
test -n "${RUNNER_TEMP:-}" || { echo 'This helper is for isolated CI runners.' >&2; exit 1; }
# Xvfb has no DRI3/DMABUF compositor. Keep these virtual-display settings out of the application.
export LIBGL_ALWAYS_SOFTWARE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
smoke_root=$(mktemp -d "$RUNNER_TEMP/bumblebee-smoke.XXXXXXXX")
export XDG_DATA_HOME="$smoke_root/data"
export XDG_CONFIG_HOME="$smoke_root/config"
export XDG_RUNTIME_DIR="$smoke_root/runtime"
mkdir -m 700 -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
unset GNOME_KEYRING_CONTROL
# DBus-activated desktop helpers need the same isolated state and Xvfb display.
dbus-update-activation-environment DISPLAY XAUTHORITY XDG_DATA_HOME XDG_CONFIG_HOME XDG_RUNTIME_DIR
# Starting and then unlocking separate daemons races DBus activation on a new login.
# One foreground daemon owns both the collection and the bus; do not let a client
# autoactivate a second daemon before this one has registered its name.
printf '%s' 'temporary-ci-keyring' | gnome-keyring-daemon --foreground --unlock --components=secrets >"$smoke_root/keyring.log" 2>&1 &
keyring_pid=$!
trap 'kill "$keyring_pid" 2>/dev/null || true; rm -rf "$smoke_root"' EXIT
keyring_ready=false
for attempt in $(seq 1 100); do
  if dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets | grep -q 'boolean true' &&
     dbus-send --session --print-reply --dest=org.freedesktop.secrets /org/freedesktop/secrets org.freedesktop.Secret.Service.ReadAlias string:default | grep -q '/org/freedesktop/secrets/collection/'; then
    keyring_ready=true
    break
  fi
  kill -0 "$keyring_pid" 2>/dev/null || break
  sleep 0.1
done
if [ "$keyring_ready" != true ]; then
  cat "$smoke_root/keyring.log" >&2
  echo 'The isolated CI keyring did not create a default collection.' >&2
  exit 1
fi
if [ "$#" -eq 0 ]; then
  set -- bumblebee-desktop
fi
timeout 120s "$@" --smoke-test
