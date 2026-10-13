import Foundation
import Combine
import AppKit

@MainActor
final class WorkspaceStore: ObservableObject {
    @Published private(set) var counter: UInt64 = 0
    @Published private(set) var revision: UInt64 = 0
    @Published private(set) var workspaceName: String?
    @Published private(set) var workspaceRoot: String?
    @Published private(set) var recentWorkspaces: [RecentWorkspace] = []
    @Published private(set) var searchQuery = ""
    @Published private(set) var searchResults: [SearchResult] = []
    @Published private(set) var searchInProgress = false
    @Published private(set) var directories: [String] = []
    @Published private(set) var sidebarRevision: UInt64 = 0
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
    @Published private(set) var externalChangeIDs: Set<String> = []
    @Published private(set) var activeDocument: DocumentSnapshot?
    @Published private(set) var editorText = ""
    private(set) var editorContentRevision: UInt64 = 0
    private(set) var editorDocumentIdentity: UInt64 = 0
    @Published private(set) var errorMessage: String?

    struct Relocation: Equatable {
        let source: String
        let target: String
        func path(_ path: String) -> String {
            if path == source { return target }
            if path.hasPrefix(source + "/") { return target + path.dropFirst(source.count) }
            return path
        }
    }
    @Published private(set) var pendingRelocation: Relocation?
    private(set) var backendBusy = false
    private let core: WorkspaceApplication
    let diagnosticsEnabled: Bool
    private let operationQueue: DispatchQueue
    private var deferredActions: [() -> Void] = []
    private var committedRelocation: Relocation?
    private var failedRelocation: Relocation?
    private var relocationFailure: String?
    private var protectedPaths: Set<String> = []
    private var sidebarDragging = false
    private var sidebarRenaming = false

    init(core: WorkspaceApplication = WorkspaceApplication(),
         operationQueue: DispatchQueue = DispatchQueue(label: "workspace.backend", qos: .userInitiated),
         persistence: Bool = true) {
        self.core = core
        self.diagnosticsEnabled = core.devtoolsAvailable()
        self.operationQueue = operationQueue
        do {
            apply(try persistence ? core.enablePersistence() : core.snapshot())
        } catch {
            errorMessage = error.localizedDescription
            if let snapshot = try? core.snapshot() { apply(snapshot) }
        }
    }

    func openDevtools() {
        if !core.openDevtools() { errorMessage = "Developer Tools are busy. Try again after the current operation finishes." }
    }

    func presentationPath(_ path: String) -> String { pendingRelocation?.path(path) ?? path }

    var sidebarDirectories: [String] { directories.map(presentationPath) }
    var sidebarDocuments: [DocumentSummary] {
        documents.map { document in
            var displayed = document
            displayed.id = presentationPath(document.id)
            displayed.title = displayed.id.split(separator: "/").last.map(String.init) ?? displayed.id
            return displayed
        }
    }
    var sidebarSearchResults: [SearchResult] {
        searchResults.map { result in
            var displayed = result
            displayed.id = presentationPath(result.id)
            displayed.title = displayed.id.split(separator: "/").last.map(String.init) ?? displayed.id
            return displayed
        }
    }

    func canMoveEntry(_ source: String, directory: String) -> Bool {
        guard pendingRelocation == nil else { return false }
        let name = source.split(separator: "/").last.map(String.init) ?? source
        let target = directory.isEmpty ? name : directory + "/" + name
        return target != source && !target.hasPrefix(source + "/") && !protectedPaths.contains(target)
    }

    func beginSidebarDrag() { sidebarDragging = true }
    func endSidebarDrag() { sidebarDragging = false }
    func setSidebarRenaming(_ value: Bool) { sidebarRenaming = value }

    private func deferUntilReady(_ action: @escaping () -> Void) -> Bool {
        guard backendBusy else { return false }
        deferredActions.append(action)
        return true
    }

    private func deferredPath(_ path: String) -> String {
        if documents.contains(where: { $0.id == path }) || directories.contains(path) { return path }
        if let failedRelocation {
            let original = Relocation(source: failedRelocation.target, target: failedRelocation.source).path(path)
            if documents.contains(where: { $0.id == original }) || directories.contains(original) { return original }
        }
        return committedRelocation?.path(path) ?? path
    }

    /// Serialize expensive FFI snapshots and filesystem work away from AppKit.
    /// Edits typed while a job runs stay in the editor and are flushed under the final path.
    private func performInBackground(relocation: Relocation? = nil, clearError: Bool = true, _ operation: @escaping @Sendable (WorkspaceApplication) throws -> WorkspaceSnapshot?) {
        guard !backendBusy else { return }
        let document = activeDocument
        let submittedText = editorText
        let core = core
        backendBusy = true
        operationQueue.async { [weak self] in
            var snapshot: WorkspaceSnapshot?
            var failure: String?
            do {
                if let document, submittedText != document.content {
                    snapshot = try core.changeDocument(id: document.id, content: submittedText)
                }
                if let outcome = try operation(core) { snapshot = outcome }
            } catch {
                failure = error.localizedDescription
                snapshot = try? core.snapshot()
            }
            let finishedSnapshot = snapshot
            let finishedFailure = failure
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                let preserveEdits = self.editorText != submittedText
                if let snapshot = finishedSnapshot { self.apply(snapshot, preserveEditorText: preserveEdits) }
                if let relocation {
                    if finishedFailure == nil { self.committedRelocation = relocation }
                    else { self.failedRelocation = relocation; self.relocationFailure = finishedFailure }
                    if self.pendingRelocation == relocation { self.pendingRelocation = nil }
                }
                if let failure = finishedFailure { self.errorMessage = failure }
                else if clearError && finishedSnapshot != nil { self.errorMessage = nil }
                self.backendBusy = false
                if preserveEdits, let active = self.activeDocument, self.editorText != active.content {
                    self.performInBackground(clearError: false) { _ in nil }
                }
                while !self.backendBusy && !self.deferredActions.isEmpty {
                    self.deferredActions.removeFirst()()
                }
                if let failure = self.relocationFailure { self.errorMessage = failure }
            }
        }
    }

    func increment() {
        if deferUntilReady({ [weak self] in self?.increment() }) { return }
        do {
            apply(try core.increment())
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func setSearchQuery(_ query: String) {
        if deferUntilReady({ [weak self] in self?.setSearchQuery(query) }) { return }
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
        if deferUntilReady({ [weak self] in self?.openWorkspace(url) }) { return }
        do {
            try flushLocalEdits()
            apply(try core.openWorkspace(path: url.path))
            committedRelocation = nil
            failedRelocation = nil
            relocationFailure = nil
            selectedSearchResultIndex = nil
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func refreshWorkspace() {
        if deferUntilReady({ [weak self] in self?.refreshWorkspace() }) { return }
        performInBackground { try $0.refreshWorkspace() }
    }

    @discardableResult
    func moveEntry(_ path: String, directory: String) -> Bool {
        guard canMoveEntry(path, directory: directory) else { return false }
        let name = path.split(separator: "/").last.map(String.init) ?? path
        let target = directory.isEmpty ? name : directory + "/" + name
        let relocation = Relocation(source: path, target: target)
        failedRelocation = nil
        relocationFailure = nil
        pendingRelocation = relocation
        errorMessage = nil
        selectedSearchResultIndex = nil
        let requestedRoot = workspaceRoot
        let action: () -> Void = { [weak self] in
            guard let self else { return }
            guard self.workspaceRoot == requestedRoot else {
                if self.pendingRelocation == relocation { self.pendingRelocation = nil }
                self.errorMessage = "Workspace changed before the move could be confirmed"
                return
            }
            self.performInBackground(relocation: relocation) { try $0.moveEntry(path: path, directory: directory) }
        }
        if !deferUntilReady({ action() }) { action() }
        return true
    }

    @discardableResult
    func renameEntry(_ path: String, newName: String) -> Bool {
        guard pendingRelocation == nil else { return false }
        guard !newName.isEmpty, newName != ".", newName != "..",
              !newName.contains("/"), !newName.contains("\\") else {
            errorMessage = "Enter a single file or folder name"
            return false
        }
        let parent = path.split(separator: "/").dropLast().joined(separator: "/")
        let target = parent.isEmpty ? newName : parent + "/" + newName
        if target == path { errorMessage = nil; return true }
        guard !protectedPaths.contains(target) else {
            errorMessage = "An entry with that name already exists"
            return false
        }
        let relocation = Relocation(source: path, target: target)
        failedRelocation = nil
        relocationFailure = nil
        pendingRelocation = relocation
        errorMessage = nil
        selectedSearchResultIndex = nil
        let requestedRoot = workspaceRoot
        let action: () -> Void = { [weak self] in
            guard let self else { return }
            guard self.workspaceRoot == requestedRoot else {
                if self.pendingRelocation == relocation { self.pendingRelocation = nil }
                self.errorMessage = "Workspace changed before the rename could be confirmed"
                return
            }
            self.performInBackground(relocation: relocation) { try $0.renameEntry(path: path, newName: newName) }
        }
        if !deferUntilReady({ action() }) { action() }
        return true
    }

    func createEntry(_ path: String, directory: Bool) {
        if deferUntilReady({ [weak self] in self?.createEntry(path, directory: directory) }) { return }
        do {
            try flushLocalEdits()
            apply(try core.createEntry(path: path, directory: directory))
            errorMessage = nil
            if !directory { openDocument(path) }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func pollWorkspaceChanges() {
        guard workspaceName != nil, !backendBusy, !sidebarRenaming else { return }
        if sidebarDragging && NSEvent.pressedMouseButtons & 1 != 0 { return }
        sidebarDragging = false
        performInBackground { try $0.pollWorkspaceChanges() }
    }

    func pollSearchResults() {
        guard searchInProgress, !backendBusy, !sidebarRenaming else { return }
        if sidebarDragging && NSEvent.pressedMouseButtons & 1 != 0 { return }
        performInBackground { try $0.pollSearchResults() }
    }

    func openDocument(_ id: String) {
        if deferUntilReady({ [weak self] in guard let self else { return }; self.openDocument(self.deferredPath(id)) }) { return }
        do {
            try flushLocalEdits()
            if searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let opened = try core.openDocumentFast(id: id)
                revision = opened.revision
                if activeDocument?.id != opened.document.id { editorDocumentIdentity &+= 1 }
                activeDocument = opened.document
                if editorText != opened.document.content {
                    editorContentRevision &+= 1
                    editorText = opened.document.content
                }
            } else {
                apply(try core.openDocument(id: id))
            }
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func openSearchResult(_ result: SearchResult, at index: Int) {
        if deferUntilReady({ [weak self] in
            guard let self else { return }
            let path = self.deferredPath(result.id)
            if let updatedIndex = self.searchResults.firstIndex(where: { $0.id == path && $0.location == result.location }) {
                self.openSearchResult(self.searchResults[updatedIndex], at: updatedIndex)
            } else { self.openSearchFile(path) }
        }) { return }
        openDocument(result.id)
        guard activeDocument?.id == result.id else { return }
        selectedSearchLocation = result.location
        selectedSearchResultIndex = index
        searchNavigationRevision &+= 1
    }

    func openSearchFile(_ id: String) {
        if deferUntilReady({ [weak self] in guard let self else { return }; self.openSearchFile(self.deferredPath(id)) }) { return }
        openDocument(id)
        guard activeDocument?.id == id else { return }
        selectedSearchLocation = nil
        selectedSearchResultIndex = nil
        searchNavigationRevision &+= 1
    }

    func changeDocument(_ content: String) {
        guard let id = activeDocument?.id, content != editorText else { return }
        editorContentRevision &+= 1
        editorText = content
        selectedSearchResultIndex = nil
        // Never wait on the FFI mutex while a relocation or reconciliation runs.
        if backendBusy { return }
        do {
            apply(try core.changeDocument(id: id, content: content))
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func saveDocument() {
        if deferUntilReady({ [weak self] in self?.saveDocument() }) { return }
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
        if deferUntilReady({ [weak self] in self?.reloadDocument() }) { return }
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
        if deferUntilReady({ [weak self] in self?.overwriteDocument() }) { return }
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

    private func apply(_ snapshot: WorkspaceSnapshot, preserveEditorText: Bool = false) {
        let diagnosticStarted: UInt64? = core.devtoolsEnabled() ? DispatchTime.now().uptimeNanoseconds : nil
        defer {
            if let started = diagnosticStarted {
                core.diagnosticSnapshotApplied(operation: snapshot.diagnosticOperation, revision: snapshot.revision, durationNs: DispatchTime.now().uptimeNanoseconds - started)
            }
        }
        let pathsChanged = directories != snapshot.directories || (documents.count != snapshot.documents.count || zip(documents, snapshot.documents).contains { $0.id != $1.id })
        let sidebarChanged = searchQuery != snapshot.searchQuery
            || searchResults != snapshot.searchResults
            || directories != snapshot.directories
            || documents != snapshot.documents
        revision = snapshot.revision
        if counter != snapshot.counter { counter = snapshot.counter }
        if workspaceName != snapshot.workspaceName { workspaceName = snapshot.workspaceName }
        if workspaceRoot != snapshot.workspaceRoot { workspaceRoot = snapshot.workspaceRoot }
        if recentWorkspaces != snapshot.recentWorkspaces { recentWorkspaces = snapshot.recentWorkspaces }
        if searchQuery != snapshot.searchQuery { searchQuery = snapshot.searchQuery }
        if searchResults != snapshot.searchResults { searchResults = snapshot.searchResults }
        if searchInProgress != snapshot.searchInProgress { searchInProgress = snapshot.searchInProgress }
        if directories != snapshot.directories { directories = snapshot.directories }
        if documents != snapshot.documents { documents = snapshot.documents }
        if pathsChanged {
            var protected = Set(snapshot.directories)
            for document in snapshot.documents {
                let parts = document.id.split(separator: "/")
                for count in 1...parts.count { protected.insert(parts.prefix(count).joined(separator: "/")) }
            }
            protectedPaths = protected
        }
        let changedIDs = Set(snapshot.documents.lazy.filter(\.externalChange).map(\.id))
        if changedIDs != externalChangeIDs { externalChangeIDs = changedIDs }
        if activeDocument?.id != snapshot.activeDocument?.id {
            let relocated = activeDocument.map { pendingRelocation?.path($0.id) == snapshot.activeDocument?.id } ?? false
            if !relocated { editorDocumentIdentity &+= 1 }
        }
        if activeDocument != snapshot.activeDocument { activeDocument = snapshot.activeDocument }
        let content = snapshot.activeDocument?.content ?? ""
        if !preserveEditorText && content != editorText {
            editorContentRevision &+= 1
            editorText = content
        }
        if sidebarChanged { sidebarRevision &+= 1 }
    }
}
