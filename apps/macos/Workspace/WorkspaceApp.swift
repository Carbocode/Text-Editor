import SwiftUI
import AppKit

@main
struct NativeWorkspaceApp: App {
    @NSApplicationDelegateAdaptor(WorkspaceAppDelegate.self) private var delegate
    private var store: WorkspaceStore { delegate.store }

    var body: some Scene {
        Window("Native Workspace", id: "home") {
            HomeView(store: store)
                .modifier(SystemOpenPresentation(delegate: delegate))
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 640, height: 560)
        .defaultLaunchBehavior(.presented)
        Window("Workspace", id: "workspace") {
            ContentView(store: store)
                .modifier(SystemOpenPresentation(delegate: delegate))
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1000, height: 700)
        .defaultLaunchBehavior(.suppressed)
        .commands { WorkspaceCommands(store: store) }
    }
}

private struct WorkspaceCommands: Commands {
    @ObservedObject var store: WorkspaceStore

    var body: some Commands {
        if store.diagnosticsEnabled {
            CommandMenu("Developer") {
                Button("Open Developer Tools") { store.openDevtools() }
                    .keyboardShortcut("d", modifiers: [.command, .option])
            }
        }
        CommandGroup(replacing: .newItem) { }
        CommandGroup(replacing: .saveItem) {
            Button("Save All") { store.saveAllDocuments() }
                .keyboardShortcut("s", modifiers: [.command, .shift])
            Button("Save") { store.saveDocument() }
                .keyboardShortcut("s", modifiers: .command)
                .disabled(!store.canSaveActiveDocument)
        }
    }
}
