import Foundation

@MainActor
final class WorkspaceStore: ObservableObject {
    @Published private(set) var counter: UInt64 = 0
    @Published private(set) var revision: UInt64 = 0
    @Published private(set) var workspaceName: String?
    @Published private(set) var recentWorkspaces: [RecentWorkspace] = []
    @Published private(set) var documents: [DocumentSummary] = []
    @Published private(set) var activeDocument: DocumentSnapshot?
    @Published private(set) var editorText = ""
    @Published private(set) var errorMessage: String?

    private let core = WorkspaceApplication()

    init() {
        do {
            apply(try core.enablePersistence())
        } catch {
            errorMessage = error.localizedDescription
            if let snapshot = try? core.snapshot() { apply(snapshot) }
        }
    }

    func increment() {
        do {
            apply(try core.increment())
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var hasDirtyActiveDocument: Bool {
        guard let activeDocument else { return false }
        return activeDocument.dirty || editorText != activeDocument.content
    }

    func openWorkspace(_ url: URL) {
        do {
            try flushLocalEdits()
            apply(try core.openWorkspace(path: url.path))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func openDocument(_ id: String) {
        do {
            try flushLocalEdits()
            apply(try core.openDocument(id: id))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func changeDocument(_ content: String) {
        guard let id = activeDocument?.id, content != editorText else { return }
        editorText = content
        do {
            apply(try core.changeDocument(id: id, content: content))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func saveDocument() {
        guard let id = activeDocument?.id else { return }
        do {
            try flushLocalEdits()
            apply(try core.saveDocument(id: id))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func flushLocalEdits() throws {
        guard let activeDocument, editorText != activeDocument.content else { return }
        apply(try core.changeDocument(id: activeDocument.id, content: editorText))
    }

    private func apply(_ snapshot: WorkspaceSnapshot) {
        revision = snapshot.revision
        counter = snapshot.counter
        workspaceName = snapshot.workspaceName
        recentWorkspaces = snapshot.recentWorkspaces
        documents = snapshot.documents
        activeDocument = snapshot.activeDocument
        editorText = snapshot.activeDocument?.content ?? ""
    }
}
