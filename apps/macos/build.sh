#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
export PATH="$HOME/.cargo/bin:$PATH"
copy_executable() {
  cp "$1" "$2.tmp.$$"
  mv "$2.tmp.$$" "$2"
}
if [ "${NATIVE_WORKSPACE_DEVTOOLS_BUILD:-1}" = "1" ]; then
  cargo build -p native-workspace-ffi-swift --features native-workspace-ffi-swift/devtools -p native-workspace-devtools -p native-workspace-devtools-ui
else
  cargo build -p native-workspace-ffi-swift --no-default-features
fi
mkdir -p apps/macos/generated apps/macos/.build/NativeWorkspace.app/Contents/MacOS
mkdir -p apps/macos/.build/NativeWorkspace.app/Contents/Resources/icons
if [ "${NATIVE_WORKSPACE_DEVTOOLS_BUILD:-1}" = "1" ]; then
  copy_executable target/debug/native-workspace-devtools apps/macos/.build/NativeWorkspace.app/Contents/MacOS/native-workspace-devtools
  tools_bundle=apps/macos/.build/NativeWorkspace.app/Contents/Helpers/NativeWorkspaceDevTools.app
  mkdir -p "$tools_bundle/Contents/MacOS"
  cp apps/devtools-ui/macos/Info.plist "$tools_bundle/Contents/Info.plist"
  copy_executable target/debug/native-workspace-devtools-ui "$tools_bundle/Contents/MacOS/native-workspace-devtools-ui"
  # Existing editor processes may still use the previous sibling lookup.
  legacy_launcher=apps/macos/.build/NativeWorkspace.app/Contents/MacOS/native-workspace-devtools-ui
  cp apps/devtools-ui/macos/launch.sh "$legacy_launcher.tmp.$$"
  chmod +x "$legacy_launcher.tmp.$$"
  mv "$legacy_launcher.tmp.$$" "$legacy_launcher"
fi
cp apps/macos/Info.plist apps/macos/.build/NativeWorkspace.app/Contents/Info.plist
cp assets/material-icon-theme/png/*.png apps/macos/.build/NativeWorkspace.app/Contents/Resources/icons/
cp assets/material-icon-theme/manifest.json apps/macos/.build/NativeWorkspace.app/Contents/Resources/icons/
cargo run --quiet -p native-workspace-ffi-swift --bin uniffi-bindgen -- generate \
  --library target/debug/libnative_workspace_ffi_swift.dylib \
  --language swift --out-dir apps/macos/generated
editor_output=$(mktemp apps/macos/.build/NativeWorkspace.XXXXXX)
trap '[ ! -f "$editor_output" ] || unlink "$editor_output"' EXIT
swiftc -parse-as-library -I apps/macos/generated \
  -Xcc -fmodule-map-file=apps/macos/generated/native_workspace_ffi_swiftFFI.modulemap \
  apps/macos/generated/native_workspace_ffi_swift.swift \
  apps/macos/Workspace/*.swift \
  target/debug/libnative_workspace_ffi_swift.a \
  -o "$editor_output"
mv "$editor_output" apps/macos/.build/NativeWorkspace.app/Contents/MacOS/NativeWorkspace
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
swiftc -parse-as-library \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/IconSmoke.swift \
  -o apps/macos/.build/IconSmoke
apps/macos/.build/IconSmoke
swiftc -parse-as-library \
  apps/macos/Workspace/SidebarInlineRename.swift \
  apps/macos/Workspace/SidebarInteractionState.swift \
  apps/macos/Workspace/WorkspaceIcons.swift \
  apps/macos/WorkspaceTests/SidebarInteractionSmoke.swift \
  -o apps/macos/.build/SidebarInteractionSmoke
apps/macos/.build/SidebarInteractionSmoke
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
