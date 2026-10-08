import Foundation

@main
struct BridgeSmoke {
    static func main() throws {
        let app = WorkspaceApplication()
        let before = try app.snapshot()
        precondition(before.counter == 0 && before.revision == 0)
        let after = try app.increment()
        precondition(after.counter == 1 && after.revision == 1)
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("README.md")
        try "before".write(to: file, atomically: true, encoding: .utf8)
        let listed = try app.openWorkspace(path: directory.path)
        precondition(listed.documents.count == 1 && listed.documents[0].id == "README.md")
        precondition(listed.recentWorkspaces.count == 1)
        precondition(URL(fileURLWithPath: listed.recentWorkspaces[0].root).lastPathComponent == directory.lastPathComponent)
        let matching = try app.setSearchQuery(query: "read")
        precondition(matching.searchResults.count == 1 && matching.searchResults[0].id == "README.md")
        let missing = try app.setSearchQuery(query: "missing")
        precondition(missing.searchResults.isEmpty)
        _ = try app.setSearchQuery(query: "")
        let opened = try app.openDocument(id: "README.md")
        precondition(opened.activeDocument?.content == "before")
        let changed = try app.changeDocument(id: "README.md", content: "after")
        precondition(changed.activeDocument?.dirty == true)
        let saved = try app.saveDocument(id: "README.md")
        precondition(saved.activeDocument?.dirty == false)
        let contentMatch = try app.setSearchQuery(query: "after")
        precondition(contentMatch.searchResults.count == 1)
        precondition(contentMatch.searchResults[0].snippet == "after")
        let diskContent = try String(contentsOf: file, encoding: .utf8)
        precondition(diskContent == "after")
        print("Swift → UniFFI → Rust workspace smoke test passed")
    }
}
