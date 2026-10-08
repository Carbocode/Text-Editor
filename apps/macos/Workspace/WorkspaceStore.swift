import Foundation

@MainActor
final class WorkspaceStore: ObservableObject {
    @Published private(set) var counter: UInt64 = 0
    @Published private(set) var revision: UInt64 = 0
    @Published private(set) var workspaceName: String?
    @Published private(set) var recentWorkspaces: [RecentWorkspace] = []
    @Published private(set) var searchQuery = ""
    @Published private(set) var searchResults: [SearchResult] = []
    @Published private(set) var selectedSearchLocation: SearchLocation?
    @Published private(set) var selectedSearchResultIndex: Int?
    @Published private(set) var searchNavigationRevision: UInt64 = 0

    var selectedSearchRange: NSRange? {
        guard let location = selectedSearchLocation,
              let start = Int(exactly: location.startUtf16),
              let end = Int(exactly: location.endUtf16), end >= start else { return nil }
        return NSRange(location: start, length: end - start)
    }
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

    func setSearchQuery(_ query: String) {
        guard query != searchQuery else { return }
        do {
            apply(try core.setSearchQuery(query: query))
            selectedSearchResultIndex = nil
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
            selectedSearchResultIndex = nil
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func refreshWorkspace() {
        updateWorkspace { try core.refreshWorkspace() }
    }

    func pollWorkspaceChanges() {
        updateWorkspace { try core.pollWorkspaceChanges() }
    }

    private func updateWorkspace(_ refresh: () throws -> WorkspaceSnapshot) {
        guard workspaceName != nil else { return }
        do {
            try flushLocalEdits()
            let snapshot = try refresh()
            if snapshot.revision != revision {
                if snapshot.activeDocument?.content != activeDocument?.content {
                    selectedSearchLocation = nil
                    selectedSearchResultIndex = nil
                    searchNavigationRevision &+= 1
                }
                apply(snapshot)
                errorMessage = nil
            }
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

    func openSearchResult(_ result: SearchResult, at index: Int) {
        openDocument(result.id)
        guard activeDocument?.id == result.id else { return }
        selectedSearchLocation = result.location
        selectedSearchResultIndex = index
        searchNavigationRevision &+= 1
    }

    func openSearchFile(_ id: String) {
        openDocument(id)
        guard activeDocument?.id == id else { return }
        selectedSearchLocation = nil
        selectedSearchResultIndex = nil
        searchNavigationRevision &+= 1
    }

    func changeDocument(_ content: String) {
        guard let id = activeDocument?.id, content != editorText else { return }
        editorText = content
        selectedSearchResultIndex = nil
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

    func reloadDocument() {
        guard let id = activeDocument?.id else { return }
        do {
            apply(try core.reloadDocument(id: id))
            selectedSearchLocation = nil
            selectedSearchResultIndex = nil
            searchNavigationRevision &+= 1
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func overwriteDocument() {
        guard let id = activeDocument?.id else { return }
        do {
            try flushLocalEdits()
            apply(try core.overwriteDocument(id: id))
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
        searchQuery = snapshot.searchQuery
        searchResults = snapshot.searchResults
        documents = snapshot.documents
        activeDocument = snapshot.activeDocument
        editorText = snapshot.activeDocument?.content ?? ""
    }
}
