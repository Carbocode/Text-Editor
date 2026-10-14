#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
sh apps/macos/build.sh
# Launch the newly built executable even when an older session is still open.
# Preserve explicit development overrides when crossing the launcher boundary.
set -- open -n -W
for name in NATIVE_WORKSPACE_STATE_DB NATIVE_WORKSPACE_DIAGNOSTICS_DIR NATIVE_WORKSPACE_DEVTOOLS NATIVE_WORKSPACE_DEVTOOLS_UI NATIVE_WORKSPACE_DEVTOOLS_SERVICE; do
  if value=$(printenv "$name"); then
    set -- "$@" --env "$name=$value"
  fi
done
exec "$@" "$PWD/apps/macos/.build/NativeWorkspace.app"
