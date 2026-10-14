import SwiftUI
import AppKit

@main
struct NativeWorkspaceApp: App {
    @StateObject private var store = WorkspaceStore()

    var body: some Scene {
        Window("Native Workspace", id: "home") {
            HomeView(store: store)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 640, height: 560)
        .defaultLaunchBehavior(.presented)
        Window("Workspace", id: "workspace") {
            ContentView(store: store)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1000, height: 700)
        .defaultLaunchBehavior(.suppressed)
        .commands {
            if store.diagnosticsEnabled {
                CommandMenu("Developer") {
                    Button("Open Developer Tools") { store.openDevtools() }
                        .keyboardShortcut("d", modifiers: [.command, .option])
                }
            }
            CommandGroup(replacing: .newItem) { }
            CommandGroup(replacing: .saveItem) {
                Button("Save") { store.saveDocument() }
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(!store.hasDirtyActiveDocument)
            }
        }
    }
}
