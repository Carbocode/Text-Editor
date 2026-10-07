#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
sh apps/macos/build.sh
exec apps/macos/.build/NativeWorkspace.app/Contents/MacOS/NativeWorkspace
