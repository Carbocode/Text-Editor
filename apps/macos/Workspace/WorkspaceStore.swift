import Foundation
import Combine
import AppKit

extension Relocation {
    func path(_ value: String) -> String {
        workspacePathRebase(value: value, source: source, target: target)
    }
}

/// AppKit presentation state. Execution, optimistic state and errors belong to Session.
@MainActor
final class WorkspaceStore: ObservableObject {
    @Published private(set) var revision: UInt64 = 0
    @Published private(set) var workspaceName: String?
    @Published private(set) var workspaceRoot: String?
    @Published private(set) var recentWorkspaces: [RecentWorkspace] = []
    @Published private(set) var searchQuery = ""
    @Published private(set) var searchFieldText = ""
    @Published private(set) var searchResults: [SearchResult] = []
    @Published private(set) var searchInProgress = false
    @Published private(set) var searchTruncated = false
    @Published private(set) var searchOptions = SearchOptions(matchCase: false, wholeWord: false, regex: false, include: "", exclude: "", useIgnoreFiles: true, onlyOpenEditors: false)
    @Published private(set) var searchError: String?
    @Published private(set) var replacementPreview: String?
    @Published private(set) var replacementOccurrences: UInt64 = 0
    @Published private(set) var replacementFiles: UInt64 = 0
    @Published var replacementText = ""
    @Published var preserveReplacementCase = false
    @Published var showReplace = false
    @Published private(set) var searchFocusRevision: UInt64 = 0
    @Published private(set) var directories: [String] = []
    @Published private(set) var documents: [DocumentSummary] = []
    @Published private(set) var fileFilterQuery = ""
    @Published private(set) var fileFilterInProgress = false
    @Published private(set) var sidebarRevision: UInt64 = 0
    @Published private(set) var selectedSearchLocation: SearchLocation?
    @Published private(set) var selectedSearchResultIndex: Int?
    @Published private(set) var searchNavigationRevision: UInt64 = 0
    @Published private(set) var openDocuments: [String] = []
    @Published private(set) var previewDocument: String?
    @Published private(set) var externalChangeIDs: Set<String> = []
    @Published private(set) var activeDocument: DocumentSnapshot?
    @Published private(set) var editorText = ""
    private(set) var editorContentRevision: UInt64 = 0
    private(set) var editorDocumentIdentity: UInt64 = 0
    @Published private(set) var errorMessage: String?
    @Published private(set) var pendingRelocation: Relocation?
    @Published private(set) var relocationSlow = false
    @Published private(set) var editingPaused = false
    private(set) var backendBusy = false
    private let core: WorkspaceApplication
    let diagnosticsEnabled: Bool
    private(set) var persistenceEnabled = false
    private var timer: Timer?
    private var lastIdlePoll = Date.distantPast
    private var sidebarDropTrace: WorkspaceDropTrace?
    private var sidebarDropTarget: String?
    private var sidebarDropAccepting = false

    var selectedSearchRange: NSRange? {
        guard let location = selectedSearchLocation,
              let start = Int(exactly: location.startUtf16),
              let end = Int(exactly: location.endUtf16), end >= start else { return nil }
        return NSRange(location: start, length: end - start)
    }
    var hasDirtyActiveDocument: Bool { activeDocument?.dirty ?? false }
    var canSaveActiveDocument: Bool {
        guard let activeDocument else { return false }
        return activeDocument.dirty || previewDocument == activeDocument.id
    }

    init(core: WorkspaceApplication = WorkspaceApplication(), persistence: Bool = true) {
        self.core = core
        diagnosticsEnabled = core.devtoolsAvailable()
        if persistence {
            persistenceEnabled = request { try core.enablePersistence() }
        } else {
            request { try core.snapshot() }
        }
        let timer = Timer(timeInterval: 0.016, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated {
                guard let self else { timer.invalidate(); return }
                let fast = self.backendBusy || self.searchInProgress || self.fileFilterInProgress
                if fast || Date().timeIntervalSince(self.lastIdlePoll) >= 0.25 {
                    self.poll()
                    self.lastIdlePoll = Date()
                }
                self.core.diagnosticUiHeartbeat()
            }
        }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    func openDevtools() {
        if !core.openDevtools() { errorMessage = "Developer Tools are busy. Try again after the current operation finishes." }
    }
    func restoreSession() { request { try core.restoreSession() } }
    @discardableResult func prepareToQuit(save: Bool) -> Bool {
        request { try core.prepareToQuit(save: save) }
    }
    func beginSidebarDrop() {
        sidebarDropTrace = core.devtoolsEnabled() ? core.beginSidebarDrop(origin: 1) : nil
        sidebarDropAccepting = true
        sidebarDropTarget = nil
    }
    func sidebarDropPreviewHidden() { sidebarDropTrace?.previewHidden() }
    func sidebarDropHandlerFinished(_ accepted: Bool) {
        sidebarDropAccepting = false
        sidebarDropTrace?.handlerFinished(accepted: accepted)
    }
    func sidebarDropRowReady(_ path: String) {
        if sidebarDropTarget == path { sidebarDropTrace?.sidebarReady() }
    }
    func canMoveFiles(_ urls: [URL], directory: String) -> Bool {
        core.canMoveFiles(paths: urls.map(\.path), directory: directory)
    }
    @discardableResult func moveFiles(_ urls: [URL], directory: String) -> Bool {
        request { try core.moveFiles(paths: urls.map(\.path), directory: directory) }
    }
    @discardableResult func openFiles(_ urls: [URL]) -> Bool {
        request { try core.openFiles(paths: urls.map(\.path)) }
    }
    @discardableResult func openPaths(_ urls: [URL]) -> Bool {
        request { try core.openPaths(paths: urls.map { $0.isFileURL ? $0.path : $0.absoluteString }) }
    }
    func canMoveEntry(_ source: String, directory: String) -> Bool {
        core.canMoveEntry(path: source, directory: directory)
    }
    func sidebarRows(search: Bool, expanded: Set<String>) -> [SidebarRow] {
        (try? core.sidebarRows(search: search, expanded: Array(expanded))) ?? []
    }
    func filterFiles(_ query: String) { request { try core.filterFiles(query: query) } }
    func setSearchQuery(_ query: String) {
        guard query != searchFieldText else { return }
        searchFieldText = query
        clearSearchSelection()
        request { try core.setSearchQuery(query: query) }
    }
    func configureSearch(_ options: SearchOptions) { clearSearchSelection(); request { try core.configureSearch(options: options) } }
    func focusSearch(replace: Bool) { showReplace = replace; searchFocusRevision &+= 1 }
    func refreshSearch() { clearSearchSelection(); request { try core.refreshSearch() } }
    func cancelSearch() { request { try core.cancelSearch() } }
    func prepareReplacement(path: String? = nil, resultIndex: Int? = nil) {
        request { try core.prepareReplacement(replacement: replacementText, preserveCase: preserveReplacementCase,
            path: path, resultIndex: resultIndex.map(UInt64.init)) }
    }
    func applyReplacement() { clearSearchSelection(); request { try core.applyReplacement() } }
    func discardReplacement() { request { try core.discardReplacement() } }
    func openWorkspace(_ url: URL) { request { try core.openWorkspace(path: url.path) } }
    func refreshWorkspace() { request { try core.refreshWorkspace() } }
    @discardableResult func moveEntry(_ path: String, directory: String) -> Bool {
        let accepted = request {
            if sidebarDropAccepting, let trace = sidebarDropTrace {
                return try core.moveEntryTraced(path: path, directory: directory, trace: trace)
            }
            return try core.moveEntry(path: path, directory: directory)
        }
        sidebarDropTarget = pendingRelocation?.target
        return accepted
    }
    @discardableResult func renameEntry(_ path: String, newName: String) -> Bool {
        request { try core.renameEntry(path: path, newName: newName) }
    }
    func deleteEntry(_ path: String) { request { try core.deleteEntry(path: path) } }
    func createEntry(_ path: String, directory: Bool) {
        if request({ try core.createEntry(path: path, directory: directory) }), !directory { openDocument(path) }
    }
    func poll() {
        do { if let update = try core.poll() { apply(update) } }
        catch { errorMessage = error.localizedDescription }
    }
    func openDocument(_ id: String, preview: Bool = false) {
        request { try preview ? core.previewDocument(id: id) : core.openDocument(id: id) }
    }
    func openSearchResult(_ result: SearchResult, at index: Int) {
        openDocument(result.id, preview: true)
        selectedSearchLocation = result.location
        selectedSearchResultIndex = index
        searchNavigationRevision &+= 1
    }
    func openSearchFile(_ id: String) {
        openDocument(id, preview: true)
        clearSearchSelection()
    }
    func keepDocumentOpen(_ id: String) { request { try core.keepDocumentOpen(id: id) } }
    func changeDocument(_ content: String) {
        guard let id = activeDocument?.id, content != editorText else { return }
        selectedSearchResultIndex = nil
        request { try core.changeDocument(id: id, content: content) }
    }
    func closeDocument(_ id: String) {
        guard documents.first(where: { $0.id == id })?.dirty == true else {
            request { try core.closeDocument(id: id) }
            return
        }
        let prompt = workspaceCloseConfirmation(value: id)
        let alert = NSAlert()
        alert.messageText = prompt.title
        alert.informativeText = prompt.body
        alert.addButton(withTitle: prompt.confirm)
        alert.addButton(withTitle: prompt.cancel)
        alert.addButton(withTitle: prompt.discard)
        switch alert.runModal() {
        case .alertFirstButtonReturn: request { try core.saveAndCloseDocument(id: id) }
        case .alertThirdButtonReturn: request { try core.discardAndCloseDocument(id: id) }
        default: break
        }
    }
    func saveAllDocuments() { request { try core.saveAllDocuments() } }
    func saveDocument() { if let id = activeDocument?.id { request { try core.saveDocument(id: id) } } }
    func reloadDocument() { if let id = activeDocument?.id { request { try core.reloadDocument(id: id) }; clearSearchSelection() } }
    func overwriteDocument() { if let id = activeDocument?.id { request { try core.overwriteDocument(id: id) } } }
    private func clearSearchSelection() {
        guard selectedSearchLocation != nil || selectedSearchResultIndex != nil else { return }
        selectedSearchLocation = nil
        selectedSearchResultIndex = nil
        searchNavigationRevision &+= 1
    }
    @discardableResult private func request(_ operation: () throws -> WorkspaceUpdate) -> Bool {
        do { apply(try operation()); return true }
        catch {
            // Composite commands may have committed steps before failing.
            if let update = try? core.snapshot() { apply(update) }
            errorMessage = error.localizedDescription
            return false
        }
    }
    private func apply(_ snapshot: WorkspaceUpdate) {
        let started = core.devtoolsEnabled() ? DispatchTime.now().uptimeNanoseconds : nil
        defer { if let started { core.diagnosticSnapshotApplied(operation: snapshot.diagnosticOperation, revision: snapshot.revision, durationNs: DispatchTime.now().uptimeNanoseconds - started) } }
        let sidebarChanged = snapshot.documents != nil || snapshot.directories != nil || snapshot.searchResults != nil
            || snapshot.fileFilterFiles != nil || fileFilterQuery != snapshot.fileFilterQuery
        revision = snapshot.revision
        if workspaceName != snapshot.workspaceName { workspaceName = snapshot.workspaceName }
        if workspaceRoot != snapshot.workspaceRoot { workspaceRoot = snapshot.workspaceRoot }
        if recentWorkspaces != snapshot.recentWorkspaces { recentWorkspaces = snapshot.recentWorkspaces }
        if searchQuery != snapshot.searchQuery { searchQuery = snapshot.searchQuery }
        if searchOptions != snapshot.searchOptions { searchOptions = snapshot.searchOptions }
        if searchError != snapshot.searchError { searchError = snapshot.searchError }
        if replacementPreview != snapshot.replacementPreview { replacementPreview = snapshot.replacementPreview }
        if replacementOccurrences != snapshot.replacementOccurrences { replacementOccurrences = snapshot.replacementOccurrences }
        if replacementFiles != snapshot.replacementFiles { replacementFiles = snapshot.replacementFiles }
        // While busy, a queued query may not be applied yet.
        if !snapshot.activity.busy && searchFieldText != snapshot.searchQuery { searchFieldText = snapshot.searchQuery }
        if let results = snapshot.searchResults { searchResults = results }
        if let folders = snapshot.directories { directories = folders }
        if let files = snapshot.documents {
            documents = files
            externalChangeIDs = Set(files.lazy.filter(\.externalChange).map(\.id))
        }
        if searchInProgress != snapshot.searchInProgress { searchInProgress = snapshot.searchInProgress }
        if searchTruncated != snapshot.searchTruncated { searchTruncated = snapshot.searchTruncated }
        if openDocuments != snapshot.openDocuments { openDocuments = snapshot.openDocuments }
        if previewDocument != snapshot.previewDocument { previewDocument = snapshot.previewDocument }
        if editorDocumentIdentity != snapshot.editorGeneration { editorDocumentIdentity = snapshot.editorGeneration; clearSearchSelection() }
        if activeDocument != snapshot.activeDocument { activeDocument = snapshot.activeDocument }
        let content = snapshot.activeDocument?.content ?? ""
        if content != editorText { editorContentRevision &+= 1; editorText = content }
        backendBusy = snapshot.activity.busy
        if editingPaused != snapshot.activity.editingPaused { editingPaused = snapshot.activity.editingPaused }
        if pendingRelocation != snapshot.activity.relocation { pendingRelocation = snapshot.activity.relocation }
        if relocationSlow != snapshot.activity.relocationSlow { relocationSlow = snapshot.activity.relocationSlow }
        if fileFilterQuery != snapshot.fileFilterQuery { fileFilterQuery = snapshot.fileFilterQuery }
        if fileFilterInProgress != snapshot.fileFilterInProgress { fileFilterInProgress = snapshot.fileFilterInProgress }
        if errorMessage != snapshot.error { errorMessage = snapshot.error }
        if sidebarChanged { sidebarRevision &+= 1 }
    }
}
