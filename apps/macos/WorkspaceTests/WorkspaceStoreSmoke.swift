import AppKit
import Foundation

@main
struct WorkspaceStoreSmoke {
    @MainActor static func main() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("dest"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try "original".write(to: root.appendingPathComponent("a.rs"), atomically: true, encoding: .utf8)
        let queue = DispatchQueue(label: "store.smoke.backend")
        let store = WorkspaceStore(operationQueue: queue, persistence: false)
        store.openWorkspace(root)
        store.openDocument("a.rs")
        store.changeDocument("first local edit")
        let documentIdentity = store.editorDocumentIdentity
        func finishBackend() {
            let deadline = Date().addingTimeInterval(5)
            while store.backendBusy && Date() < deadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            }
            precondition(!store.backendBusy, "Background backend did not finish")
        }
        queue.suspend()
        let started = Date()
        precondition(store.renameEntry("a.rs", newName: "new.rs"))
        precondition(Date().timeIntervalSince(started) < 0.1, "Inline rename blocked the main thread")
        precondition(store.presentationPath("a.rs") == "new.rs", "Rename should display immediately")
        precondition(FileManager.default.fileExists(atPath: root.appendingPathComponent("a.rs").path))
        store.changeDocument("edit typed during confirmation")
        queue.resume()
        finishBackend()
        precondition(store.activeDocument?.id == "new.rs")
        precondition(store.editorText == "edit typed during confirmation")
        precondition(store.activeDocument?.content == store.editorText)
        precondition(store.editorDocumentIdentity == documentIdentity, "Rename should preserve editor identity")
        store.saveDocument()
        let renamedContent = try String(contentsOf: root.appendingPathComponent("new.rs"), encoding: .utf8)
        precondition(renamedContent == store.editorText)

        // Introduce a collision after optimistic validation but before filesystem confirmation.
        queue.suspend()
        precondition(store.renameEntry("new.rs", newName: "collision.rs"))
        precondition(store.presentationPath("new.rs") == "collision.rs")
        try "external".write(to: root.appendingPathComponent("collision.rs"), atomically: true, encoding: .utf8)
        store.changeDocument("edit typed during rejection")
        queue.resume()
        finishBackend()
        precondition(store.pendingRelocation == nil)
        precondition(store.presentationPath("new.rs") == "new.rs", "Failed rename did not roll back")
        precondition(store.activeDocument?.id == "new.rs" && store.editorText == "edit typed during rejection")
        precondition(store.errorMessage != nil, "Backend rejection should remain visible")
        let externalContent = try String(contentsOf: root.appendingPathComponent("collision.rs"), encoding: .utf8)
        precondition(externalContent == "external")

        queue.suspend()
        let moveStarted = Date()
        precondition(store.moveEntry("new.rs", directory: "dest"))
        precondition(Date().timeIntervalSince(moveStarted) < 0.1, "Drop blocked the UI")
        precondition(store.sidebarDocuments.contains { $0.id == "dest/new.rs" }, "Moved file is not immediately in its destination branch")
        precondition(!store.sidebarDocuments.contains { $0.id == "new.rs" }, "Moved file remained in the original branch")
        precondition(FileManager.default.fileExists(atPath: root.appendingPathComponent("new.rs").path))
        store.saveDocument() // queued behind the move
        store.openSearchFile("new.rs") // its old path must resolve after confirmation
        queue.resume()
        finishBackend()
        precondition(store.activeDocument?.id == "dest/new.rs")
        precondition(store.activeDocument?.dirty == false)
        let movedContent = try String(contentsOf: root.appendingPathComponent("dest/new.rs"), encoding: .utf8)
        precondition(movedContent == "edit typed during rejection")

        queue.suspend()
        store.refreshWorkspace()
        precondition(store.backendBusy)
        precondition(store.renameEntry("dest/new.rs", newName: "final.rs"), "Rename should not wait for a background poll")
        precondition(store.presentationPath("dest/new.rs") == "dest/final.rs")
        store.changeDocument("edit while poll and rename are queued")
        queue.resume()
        finishBackend()
        precondition(store.activeDocument?.id == "dest/final.rs")
        precondition(store.editorText == "edit while poll and rename are queued")
        precondition(store.pendingRelocation == nil)

        queue.suspend()
        precondition(store.moveEntry("dest/final.rs", directory: ""))
        precondition(store.sidebarDocuments.contains { $0.id == "final.rs" })
        try "external destination".write(to: root.appendingPathComponent("final.rs"), atomically: true, encoding: .utf8)
        store.changeDocument("typing during rejected move")
        store.openSearchFile("final.rs") // click the optimistic row before confirmation
        store.saveDocument()
        queue.resume()
        finishBackend()
        precondition(store.activeDocument?.id == "dest/final.rs", "Failed move did not restore the clicked row's original path")
        precondition(store.sidebarDocuments.contains { $0.id == "dest/final.rs" })
        precondition(store.editorText == "typing during rejected move")
        precondition(store.errorMessage != nil, "Deferred actions hid the failed move error")
        let restoredContent = try String(contentsOf: root.appendingPathComponent("dest/final.rs"), encoding: .utf8)
        precondition(restoredContent == store.editorText)
        let externalDestination = try String(contentsOf: root.appendingPathComponent("final.rs"), encoding: .utf8)
        precondition(externalDestination == "external destination")

        // Typing a search query returns at once and never waits on a busy backend.
        queue.suspend()
        store.refreshWorkspace()
        let searchStarted = Date()
        store.setSearchQuery("t")
        store.setSearchQuery("typing")
        precondition(Date().timeIntervalSince(searchStarted) < 0.1, "Search typing blocked the main thread")
        precondition(store.searchFieldText == "typing", "The search field did not show typed text at once")
        queue.resume()
        let searchDeadline = Date().addingTimeInterval(5)
        while (store.backendBusy || store.searchInProgress || store.searchQuery != "typing") && Date() < searchDeadline {
            store.pollSearchResults()
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        precondition(store.searchQuery == "typing" && !store.searchInProgress, "Queued search did not finish")
        precondition(store.searchResults.contains { $0.id == "dest/final.rs" && $0.location != nil },
                     "Search did not find the saved text")
        precondition(store.searchFieldText == "typing")

        // Deletion flushes local edits, rejects dirty files and stays off the main thread.
        store.changeDocument("unsaved before deletion")
        store.deleteEntry("dest")
        finishBackend()
        precondition(store.errorMessage?.contains("Save all modified") == true)
        precondition(store.editorText == "unsaved before deletion")
        precondition(FileManager.default.fileExists(atPath: root.appendingPathComponent("dest/final.rs").path))
        store.saveDocument()
        queue.suspend()
        store.deleteEntry("dest")
        precondition(store.deletionPending && store.backendBusy)
        store.changeDocument("typing is paused during deletion")
        precondition(store.editorText == "unsaved before deletion")
        queue.resume()
        finishBackend()
        precondition(!store.deletionPending)
        precondition(store.activeDocument == nil)
        precondition(!FileManager.default.fileExists(atPath: root.appendingPathComponent("dest").path))

        let draft = SidebarRenameDraft()
        draft.begin("README.md")
        draft.changeName("preview.rs")
        draft.changeName("preview.py")
        precondition(draft.previewPath == "README.md", "Icon preview should be debounced")
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        precondition(draft.previewPath == "preview.py")
        draft.changeName("cancelled.json")
        draft.finish()
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        precondition(draft.previewPath == nil, "Cancelled icon preview leaked into another row")
        print("macOS optimistic rename/rollback/concurrent edits/deferred save/icon debounce smoke test passed")
    }
}
