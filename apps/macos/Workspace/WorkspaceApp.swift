import SwiftUI
import AppKit

@main
struct NativeWorkspaceApp: App {
    @StateObject private var store = WorkspaceStore()

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
                .onAppear {
                    DispatchQueue.main.async {
                        NSApp.activate()
                        NSApp.windows.first(where: { $0.canBecomeMain })?.makeKeyAndOrderFront(nil)
                    }
                }
        }
        .defaultLaunchBehavior(.presented)
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
