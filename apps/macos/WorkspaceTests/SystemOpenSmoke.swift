import AppKit
import Foundation

@main
struct SystemOpenSmoke {
    @MainActor static func main() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let other = root.appendingPathComponent("other", isDirectory: true)
        try FileManager.default.createDirectory(at: other, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("note ü.txt")
        try "saved".write(to: file, atomically: true, encoding: .utf8)
        let core = WorkspaceApplication()
        let store = WorkspaceStore(core: core, persistence: false)
        let delegate = WorkspaceAppDelegate(store: store)
        var presentations = 0
        func finish() {
            let deadline = Date().addingTimeInterval(5)
            while store.backendBusy && Date() < deadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            }
            precondition(!store.backendBusy, "System open did not finish")
        }
        // Delivery before SwiftUI has installed its window presenter.
        try core.delayNextBackgroundJob(milliseconds: 150)
        delegate.application(NSApp, open: [file])
        precondition(presentations == 0 && store.editingPaused)
        delegate.installPresentation { presentations += 1 }
        precondition(presentations == 1)
        finish()
        // Foundation can retain /var while Rust canonicalizes it to /private/var.
        precondition(store.workspaceRoot?.hasSuffix("/" + root.lastPathComponent) == true)
        precondition(store.activeDocument?.id == file.lastPathComponent)
        precondition(store.previewDocument == nil && store.errorMessage == nil)
        store.changeDocument("unsaved")
        delegate.application(NSApp, open: [other])
        finish()
        precondition(presentations == 2)
        precondition(store.errorMessage != nil && store.editorText == "unsaved")
        // Reopening the current folder preserves even dirty tabs.
        delegate.application(NSApp, open: [root])
        finish()
        precondition(store.errorMessage == nil && store.editorText == "unsaved")
        store.saveDocument()
        delegate.application(NSApp, open: [other])
        finish()
        precondition(store.workspaceRoot?.hasSuffix("/" + root.lastPathComponent + "/other") == true)
        precondition(store.openDocuments.isEmpty)
        delegate.application(NSApp, open: [URL(string: "https://example.test/note")!])
        finish()
        precondition(store.errorMessage != nil)
        print("macOS system-open cold/warm delivery/presentation/dirty-buffer smoke test passed")
    }
}
