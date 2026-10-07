#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p native-workspace-ffi-swift
mkdir -p apps/macos/generated apps/macos/.build/NativeWorkspace.app/Contents/MacOS
mkdir -p apps/macos/.build/NativeWorkspace.app/Contents/Resources/icons
cp apps/macos/Info.plist apps/macos/.build/NativeWorkspace.app/Contents/Info.plist
cp assets/material-icon-theme/*.png apps/macos/.build/NativeWorkspace.app/Contents/Resources/icons/
cargo run --quiet -p native-workspace-ffi-swift --bin uniffi-bindgen -- generate \
  --library target/debug/libnative_workspace_ffi_swift.dylib \
  --language swift --out-dir apps/macos/generated
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/*.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/NativeWorkspace.app/Contents/MacOS/NativeWorkspace
swiftc -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/WorkspaceTests/BridgeSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/BridgeSmoke
apps/macos/.build/BridgeSmoke
