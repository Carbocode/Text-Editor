#!/bin/sh
set -eu
tools_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../Helpers/NativeWorkspaceDevTools.app/Contents/MacOS" && pwd)
exec "$tools_dir/native-workspace-devtools-ui" "$@"
