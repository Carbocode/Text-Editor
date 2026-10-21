#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
exec cargo xtask build macos
