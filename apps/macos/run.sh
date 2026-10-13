#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
sh apps/macos/build.sh
# Launch the editor bundle through Launch Services, bringing its window forward.
# Preserve explicit development overrides when crossing the launcher boundary.
set -- open -W
for name in NATIVE_WORKSPACE_STATE_DB NATIVE_WORKSPACE_DIAGNOSTICS_DIR NATIVE_WORKSPACE_DEVTOOLS NATIVE_WORKSPACE_DEVTOOLS_UI NATIVE_WORKSPACE_DEVTOOLS_SERVICE; do
  if value=$(printenv "$name"); then
    set -- "$@" --env "$name=$value"
  fi
done
exec "$@" "$PWD/apps/macos/.build/NativeWorkspace.app"
