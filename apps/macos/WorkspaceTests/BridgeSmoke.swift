import Foundation

@main
struct BridgeSmoke {
    static func main() throws {
        let app = WorkspaceApplication()
        func state() throws -> WorkspaceUpdate { try app.snapshot() }
        func settle() throws -> WorkspaceUpdate {
            let deadline = Date().addingTimeInterval(5)
            while true {
                _ = try app.poll()
                let snapshot = try state()
                if !snapshot.activity.busy && !snapshot.searchInProgress && !snapshot.fileFilterInProgress { return snapshot }
                precondition(Date() < deadline, "Session did not settle")
                usleep(5_000)
            }
        }
        func search(_ query: String) throws -> WorkspaceUpdate {
            _ = try app.setSearchQuery(query: query)
            return try settle()
        }
        let initial = try state()
        precondition(initial.revision == 0)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("README.md")
        try "before".write(to: file, atomically: true, encoding: .utf8)
        let listed = try app.openWorkspace(path: root.path)
        precondition(listed.documents?.first?.id == "README.md")
        precondition(listed.recentWorkspaces.count == 1)
        var invalidBlocked = false
        do { _ = try app.openDocument(id: "../escape") } catch { invalidBlocked = true }
        precondition(invalidBlocked)
        let invalid = try state()
        _ = try app.poll()
        precondition(invalid.error != nil)
        let polledFailure = try state()
        precondition(polledFailure.error == invalid.error)
        _ = try app.openDocument(id: "README.md")
        let firstEdit = try app.changeDocument(id: "README.md", content: "after 💡 after")
        precondition(firstEdit.activeDocument?.dirty == true)
        // An edit does not serialize workspace paths or unchanged matches.
        let nextEdit = try app.changeDocument(id: "README.md", content: "after 💡 after!")
        precondition(nextEdit.documents == nil && nextEdit.directories == nil && nextEdit.searchResults == nil && nextEdit.gitJson == nil)
        _ = try app.saveDocument(id: "README.md")
        let saved = try String(contentsOf: file, encoding: .utf8)
        precondition(saved == "after 💡 after!")
        let found = try search("after")
        precondition(found.searchResults?.count == 2)
        precondition(found.searchResults?[1].location.startUtf16 == 9)
        precondition(found.searchResults?[1].location.endUtf16 == 14)
        let namesOnly = try search("README")
        precondition(namesOnly.searchResults?.isEmpty == true)
        _ = try app.filterFiles(query: "read")
        let filtered = try settle()
        precondition(filtered.fileFilterFiles == ["README.md"])
        _ = try app.filterFiles(query: "")
        _ = try search("")
        try "external clean".write(to: file, atomically: true, encoding: .utf8)
        _ = try app.refreshWorkspace()
        let refreshed = try settle()
        precondition(refreshed.activeDocument?.content == "external clean")
        _ = try app.changeDocument(id: "README.md", content: "local edit")
        try "external conflict".write(to: file, atomically: true, encoding: .utf8)
        var blocked = false
        do { _ = try app.saveAndCloseDocument(id: "README.md") } catch { blocked = true }
        precondition(blocked)
        let conflict = try state()
        precondition(conflict.openDocuments == ["README.md"] && conflict.activeDocument?.dirty == true)
        _ = try app.overwriteDocument(id: "README.md")
        _ = try app.createEntry(path: "dest", directory: true)
        try app.delayNextBackgroundJob(milliseconds: 200)
        let moved = try app.moveEntry(path: "README.md", directory: "dest")
        precondition(moved.activeDocument?.id == "dest/README.md" && moved.activity.busy)
        precondition(moved.editorGeneration == conflict.editorGeneration)
        _ = try app.changeDocument(id: "dest/README.md", content: "typed while moving")
        _ = try app.saveDocument(id: "dest/README.md")
        let confirmed = try settle()
        precondition(confirmed.activeDocument?.dirty == false)
        let movedText = try String(contentsOf: root.appendingPathComponent("dest/README.md"), encoding: .utf8)
        precondition(movedText == "typed while moving")
        let rows = try app.sidebarRows(search: false, expanded: ["dest"])
        precondition(rows.map(\.path) == ["dest", "dest/README.md"])
        _ = try app.saveAndCloseDocument(id: "dest/README.md")
        let closed = try state()
        precondition(closed.openDocuments.isEmpty && closed.activeDocument == nil)
        _ = try app.deleteEntry(path: "dest")
        let deleted = try settle()
        precondition(deleted.documents?.isEmpty == true && deleted.directories?.isEmpty == true)
        print("Swift → UniFFI → Session smoke test passed")
    }
}
