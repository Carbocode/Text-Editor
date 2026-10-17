import AppKit
import Foundation

@main
struct SessionRestoreSmoke {
    @MainActor static func main() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        setenv("NATIVE_WORKSPACE_STATE_DB", root.appendingPathComponent("state.sqlite3").path, 1)
        defer { unsetenv("NATIVE_WORKSPACE_STATE_DB") }
        let workspace = root.appendingPathComponent("workspace", isDirectory: true)
        let external = root.appendingPathComponent("outside.txt")
        try FileManager.default.createDirectory(at: workspace, withIntermediateDirectories: true)
        let first = workspace.appendingPathComponent("first.txt")
        let preview = workspace.appendingPathComponent("preview.txt")
        try "first".write(to: first, atomically: true, encoding: .utf8)
        try "preview".write(to: preview, atomically: true, encoding: .utf8)
        try "external".write(to: external, atomically: true, encoding: .utf8)

        func finish(_ store: WorkspaceStore) {
            let deadline = Date().addingTimeInterval(5)
            while store.backendBusy && Date() < deadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            }
            precondition(!store.backendBusy, "Session restore did not finish")
        }
        do {
            let store = WorkspaceStore()
            store.openWorkspace(workspace)
            store.openDocument("first.txt")
            store.openDocument("preview.txt", preview: true)
            store.openFiles([external])
            finish(store)
            store.openDocument("first.txt", preview: true)
            precondition(store.errorMessage == nil)
        }
        let core = WorkspaceApplication()
        let store = WorkspaceStore(core: core)
        let delegate = WorkspaceAppDelegate(store: store)
        var presentations = 0
        delegate.installPresentation { presentations += 1 }
        delegate.applicationDidFinishLaunching(Notification(name: NSApplication.didFinishLaunchingNotification))
        finish(store)
        precondition(presentations == 1)
        precondition(store.openDocuments.count == 3 && store.openDocuments[0] == "first.txt")
        precondition(store.previewDocument == "preview.txt" && store.activeDocument?.id == "first.txt")
        precondition(store.errorMessage == nil)
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateNow)
        try core.delayNextBackgroundJob(milliseconds: 100)
        store.refreshWorkspace()
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateCancel)
        finish(store)

        let prompt = workspaceQuitConfirmation()
        func choose(_ title: String) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                func button(_ view: NSView) -> NSButton? {
                    if let candidate = view as? NSButton, candidate.title == title { return candidate }
                    return view.subviews.lazy.compactMap { button($0) }.first
                }
                guard let view = NSApp.modalWindow?.contentView, let selected = button(view) else {
                    preconditionFailure("Quit confirmation did not show button: \(title)")
                }
                selected.performClick(nil)
            }
        }
        store.changeDocument("unsaved")
        choose(prompt.cancel)
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateCancel)
        precondition(store.editorText == "unsaved")
        try "outside edit".write(to: first, atomically: true, encoding: .utf8)
        choose(prompt.confirm)
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateCancel)
        precondition(store.errorMessage != nil && store.editorText == "unsaved")
        choose(prompt.discard)
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateNow)
        let discarded = try String(contentsOf: first, encoding: .utf8)
        precondition(discarded == "outside edit")
        store.reloadDocument()
        store.changeDocument("saved before quit")
        choose(prompt.confirm)
        precondition(delegate.applicationShouldTerminate(NSApp) == .terminateNow)
        let saved = try String(contentsOf: first, encoding: .utf8)
        precondition(saved == "saved before quit")

        try FileManager.default.removeItem(at: preview)
        let partial = WorkspaceStore()
        let partialDelegate = WorkspaceAppDelegate(store: partial)
        partialDelegate.applicationDidFinishLaunching(Notification(name: NSApplication.didFinishLaunchingNotification))
        finish(partial)
        precondition(partial.openDocuments.count == 2 && partial.errorMessage != nil)
        precondition(partial.prepareToQuit(save: false))
        try "returned".write(to: preview, atomically: true, encoding: .utf8)
        let returned = WorkspaceStore()
        returned.restoreSession()
        finish(returned)
        precondition(returned.openDocuments.count == 3)

        // Finder requests received before didFinishLaunching suppress automatic restoration.
        let requested = WorkspaceStore()
        let requestedDelegate = WorkspaceAppDelegate(store: requested)
        requestedDelegate.application(NSApp, open: [external])
        requestedDelegate.applicationDidFinishLaunching(Notification(name: NSApplication.didFinishLaunchingNotification))
        finish(requested)
        precondition(requested.openDocuments == ["outside.txt"])
        let unavailable = root.appendingPathComponent("unavailable.sqlite3", isDirectory: true)
        try FileManager.default.createDirectory(at: unavailable, withIntermediateDirectories: true)
        setenv("NATIVE_WORKSPACE_STATE_DB", unavailable.path, 1)
        let failed = WorkspaceStore()
        let failedDelegate = WorkspaceAppDelegate(store: failed)
        failedDelegate.applicationDidFinishLaunching(Notification(name: NSApplication.didFinishLaunchingNotification))
        precondition(!failed.persistenceEnabled && !failed.backendBusy && failed.errorMessage != nil)
        print("macOS session restore/preview/external tabs/partial restore/launch precedence/quit confirmation/storage failure smoke test passed")
    }
}
