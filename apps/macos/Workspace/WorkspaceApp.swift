import SwiftUI

@main
struct NativeWorkspaceApp: App {
    @StateObject private var store = WorkspaceStore()

    var body: some Scene {
        WindowGroup {
            ContentView(store: store)
        }
        .commands {
            CommandGroup(replacing: .newItem) { }
            CommandGroup(replacing: .saveItem) {
                Button("Save") { store.saveDocument() }
                    .keyboardShortcut("s", modifiers: .command)
                    .disabled(!store.hasDirtyActiveDocument)
            }
        }
    }
}
