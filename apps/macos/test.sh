#!/bin/sh
# Compile and run Swift smoke tests against an already built bridge.
set -eu
cd "$(dirname "$0")/../.."
if [ ! -f apps/macos/generated/native_workspace_ffi_swift.swift ] || [ ! -f target/debug/libnative_workspace_ffi_swift.a ]; then
  echo "Build prerequisites first: cargo xtask build macos (or use cargo xtask test macos)." >&2
  exit 1
fi
mkdir -p apps/macos/.build
swiftc -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/WorkspaceTests/BridgeSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/BridgeSmoke
apps/macos/.build/BridgeSmoke
swiftc -parse-as-library \
  -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/EditorMinimapRaster.swift \
  apps/macos/Workspace/SyntaxHighlighting.swift \
  apps/macos/Workspace/NoWrapTextEditor.swift \
  apps/macos/WorkspaceTests/EditorLayoutSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/EditorLayoutSmoke
apps/macos/.build/EditorLayoutSmoke
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/IconSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/IconSmoke
apps/macos/.build/IconSmoke
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/SidebarInlineRename.swift \
  apps/macos/Workspace/SidebarInteractionState.swift \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/SidebarInteractionSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/SidebarInteractionSmoke
apps/macos/.build/SidebarInteractionSmoke
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/SidebarNativeDragSource.swift \
  apps/macos/Workspace/SidebarNativeDropTarget.swift \
  apps/macos/Workspace/ExternalFileDrop.swift \
  apps/macos/Workspace/SidebarInteractionState.swift \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/NativeDragPointerSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/NativeDragPointerSmoke
apps/macos/.build/NativeDragPointerSmoke
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/WorkspaceStore.swift \
  apps/macos/Workspace/SidebarInteractionState.swift \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/WorkspaceStoreSmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/WorkspaceStoreSmoke
apps/macos/.build/WorkspaceStoreSmoke
set --
for source in apps/macos/Workspace/*.swift; do
  case "$source" in */WorkspaceApp.swift) continue ;; esac
  set -- "$@" "$source"
done
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  "$@" apps/macos/WorkspaceTests/WorkspaceDragLatencySmoke.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o apps/macos/.build/WorkspaceDragLatencySmoke
apps/macos/.build/WorkspaceDragLatencySmoke
