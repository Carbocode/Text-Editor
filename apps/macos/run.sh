#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
sh apps/macos/build.sh
exec sh apps/macos/start.sh
