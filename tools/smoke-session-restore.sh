#!/bin/sh
# Real seed/restart launches. Always use disposable files and metadata.
set -eu
cd "$(dirname "$0")/.."
frontend=${1:?Pass gnome or kde}
case "$frontend" in gnome|kde) ;; *) echo 'Pass gnome or kde' >&2; exit 1 ;; esac
fixture=$(mktemp -d "${TMPDIR:-/tmp}/native-workspace-session.XXXXXX")
trap 'rm -rf "$fixture"' EXIT HUP INT TERM
fixture=$(cd "$fixture" && pwd -P)
workspace="$fixture/workspace"
mkdir -p "$workspace/src"
printf 'Session restore fixture\n' > "$workspace/README.md"
printf 'fn main() {}\n' > "$workspace/src/main.rs"
printf 'External file\n' > "$workspace.external.txt"
export NATIVE_WORKSPACE_STATE_DB="$fixture/state.sqlite3"
export NATIVE_WORKSPACE_NEW_INSTANCE=1
launch() {
    if [ "$frontend" = gnome ]; then
        NATIVE_WORKSPACE_SMOKE_RESTORE="$workspace" NATIVE_WORKSPACE_SMOKE_RESTORE_MODE="$1" \
            target/debug/native-workspace-gnome
    elif [ "$1" = priority ]; then
        target/debug/native-workspace-kde --smoke-restore="$1" --restore-fixture="$workspace" "$workspace.external.txt"
    else
        target/debug/native-workspace-kde --smoke-restore="$1" --restore-fixture="$workspace"
    fi
}
launch seed
launch restore
if [ "$frontend" = kde ]; then
    saved=$(cat "$workspace/README.md")
    launch discard
    [ "$(cat "$workspace/README.md")" = "$saved" ]
fi
mv "$workspace/src/main.rs" "$fixture/main.rs"
launch missing
mv "$fixture/main.rs" "$workspace/src/main.rs"
launch restore
launch priority
printf '%s session restart smoke suite passed\n' "$frontend"
