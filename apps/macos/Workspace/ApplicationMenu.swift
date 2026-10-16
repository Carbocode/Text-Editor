import AppKit
import SwiftUI

struct ApplicationMenu: View {
    @ObservedObject var store: WorkspaceStore
    @Environment(\.openWindow) private var openWindow
    let openFolder: () -> Void

    var body: some View {
        Menu {
            Button("Home", systemImage: "house") { openWindow(id: "home") }
            if store.diagnosticsEnabled {
                Button("Developer Tools", systemImage: "waveform.path.ecg") { store.openDevtools() }
            }
            Button("Open Folder…", systemImage: "folder") { openFolder() }
                .keyboardShortcut("o", modifiers: .command)
            Divider()
            Button("Quit", systemImage: "power") { NSApplication.shared.terminate(nil) }
        } label: {
            Label("Main Menu", systemImage: "ellipsis")
        }
        .labelStyle(.iconOnly)
        .menuIndicator(.hidden)
        .help("Main Menu")
        .accessibilityIdentifier("applicationMenu")
    }
}
