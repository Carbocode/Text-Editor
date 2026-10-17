import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false
    @State private var editorFontSize = NSFont.systemFontSize
    @State private var resolvingExternalChange = false
    @State private var deletePath = ""
    @State private var confirmingDelete = false
    @State private var renamePath: String?
    @State private var renameDraft = SidebarRenameDraft()
    @State private var sidebarDrag = SidebarDragState()
    @State private var creatingEntry = false
    @State private var creatingDirectory = false
    @State private var newEntryPath = ""
    @State private var sidebarTab = SidebarSection.files
    @State private var fileNameQuery = ""
    private var appliedFileNameQuery: String { store.fileFilterQuery }
    @State private var sidebarBottomBarHeight: CGFloat = 44
    @State private var sidebarRootDropHeight: CGFloat = 0
    @State private var expandedDirectories: Set<String> = []
    @State private var selectedDirectory: String?
    @State private var currentWorkspaceRoot: String?
    @State private var sidebarRows: [SidebarOutlineRow] = []
    private var sidebarIndexInProgress: Bool { store.fileFilterInProgress }
    @State private var sidebarRelocation: Relocation?

    private struct SearchMatchRow {
        let index: Int
        let result: SearchResult
    }

    private struct SearchFileGroup {
        let id: String
        let file: DocumentSummary
    }

    private struct SidebarOutlineRow: Identifiable {
        enum Kind {
            case directory(String)
            case file(SearchFileGroup)
            case match(SearchMatchRow)
        }

        let id: String
        let depth: Int
        let kind: Kind
    }

    private func rebuildSidebarIndex() {
        guard sidebarTab.hasTree else { sidebarRows = []; return }
        sidebarRows = store.sidebarRows(search: sidebarTab == .search, expanded: expandedDirectories).compactMap { row in
            if row.directory {
                return SidebarOutlineRow(id: "entry:\(row.path)", depth: Int(row.depth), kind: .directory(row.path))
            }
            if let index = row.resultIndex, let position = Int(exactly: index), store.searchResults.indices.contains(position) {
                return SidebarOutlineRow(id: "match:\(position)", depth: Int(row.depth),
                    kind: .match(SearchMatchRow(index: position, result: store.searchResults[position])))
            }
            let file = DocumentSummary(id: row.path, title: row.title, dirty: row.dirty, externalChange: row.externalChange)
            return SidebarOutlineRow(id: "entry:\(row.path)", depth: Int(row.depth), kind: .file(SearchFileGroup(id: row.path, file: file)))
        }
    }

    private func toggleDirectory(_ path: String) {
        if expandedDirectories.contains(path) { expandedDirectories.remove(path) }
        else { expandedDirectories.insert(path) }
        rebuildSidebarIndex()
    }

    @ViewBuilder private func sidebarRow(_ row: SidebarOutlineRow) -> some View {
        switch row.kind {
        case .directory(let path):
            HStack(spacing: 8) {
                Button { toggleDirectory(path) } label: {
                    Image(systemName: expandedDirectories.contains(path) || sidebarTab == .search || !appliedFileNameQuery.isEmpty
                        ? "chevron.down" : "chevron.right").font(.caption)
                }
                .buttonStyle(.plain)
                Button {
                    if renamePath != path {
                        selectedDirectory = path
                        toggleDirectory(path)
                    }
                } label: {
                    HStack(spacing: 8) {
                        SidebarEntryIcon(draft: renameDraft, originalPath: path,
                                         displayPath: path, directory: true,
                                         expanded: expandedDirectories.contains(path) || sidebarTab == .search || !appliedFileNameQuery.isEmpty)
                        entryName(path).opacity(isPendingMove(path) ? 0.5 : 1)
                        pendingMoveIndicator(path)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .padding(.leading, CGFloat(row.depth) * 14 + 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .background(nativeDragSource(path, directory: true))
            .overlay(nativeDropTarget(path))
            .overlay { SidebarDropOutline(drag: sidebarDrag, directory: path, selected: selectedDirectory == path) }
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(path); return .handled }
            .contextMenu {
                Button("Rename") { beginRename(path) }
                Button("Delete…", role: .destructive) { requestDelete(path) }
            }
        case .file(let group):
            Button {
                if renamePath != group.id {
                    selectedDirectory = nil
                    store.openSearchFile(group.id)
                }
            } label: {
                HStack(spacing: 8) {
                    SidebarEntryIcon(draft: renameDraft, originalPath: group.id,
                                     displayPath: group.id)
                    entryName(group.id).opacity(isPendingMove(group.id) ? 0.5 : 1)
                    Spacer(minLength: 0)
                    pendingMoveIndicator(group.id)
                    if store.openDocuments.contains(where: { $0 == group.id }) {
                        Image(systemName: "eye")
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Open in editor")
                            .accessibilityIdentifier("sidebarOpen:" + group.id)
                            .help("Open in editor")
                    }
                    if store.externalChangeIDs.contains(where: { $0 == group.id }) {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    }
                    if group.file.dirty || (store.activeDocument.map { $0.id } == group.id && store.hasDirtyActiveDocument) { Circle().fill(.secondary).frame(width: 6, height: 6) }
                }
                .padding(.leading, CGFloat(row.depth) * 14 + 8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .simultaneousGesture(TapGesture(count: 2).onEnded { store.openDocument(group.id) })
            .background(nativeDragSource(group.id, directory: false))
            .overlay(nativeDropTarget(parentDirectory(group.id)))
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(group.id); return .handled }
            .contextMenu {
                if sidebarTab == .search && store.showReplace {
                    Button("Replace All in File…") { store.prepareReplacement(path: group.id) }
                    Divider()
                }
                Button("Rename") { beginRename(group.id) }
                Button("Delete…", role: .destructive) { requestDelete(group.id) }
            }
            .overlay {
                if selectedDirectory == nil && store.selectedSearchResultIndex == nil
                    && store.activeDocument.map({ $0.id }) == group.id {
                    SidebarRowHighlight()
                }
            }
        case .match(let match):
            Button {
                selectedDirectory = nil
                store.openSearchResult(match.result, at: match.index)
            } label: {
                HStack(alignment: .top, spacing: 8) {
                    Text("\(match.result.location.line):\(match.result.location.column)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    Text(match.result.snippet).font(.caption).lineLimit(1)
                    Spacer(minLength: 0)
                }
                .padding(.leading, CGFloat(row.depth) * 14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .simultaneousGesture(TapGesture(count: 2).onEnded { store.keepDocumentOpen(match.result.id) })
            .contextMenu {
                Button("Replace…") { store.prepareReplacement(resultIndex: match.index) }
                Button("Replace All in File…") { store.prepareReplacement(path: match.result.id) }
            }
            .overlay {
                if selectedDirectory == nil && store.selectedSearchResultIndex == match.index {
                    SidebarRowHighlight()
                }
            }
        }
    }

    private let sidebarDragType = UTType(exportedAs: "org.nativeworkspace.sidebar-entry", conformingTo: .data)

    private func parentDirectory(_ path: String) -> String {
        workspacePathParent(value: path)
    }

    @ViewBuilder private func entryName(_ path: String) -> some View {
        if renamePath == path {
            SidebarInlineRename(name: Binding(get: { renameDraft.name }, set: { renameDraft.changeName($0) }),
                                directory: store.directories.contains(path),
                                submit: { commitRename() }, cancel: { cancelRename() })
                .frame(height: 22)
        } else {
            Text(workspacePathName(value: path))
        }
    }

    private func requestDelete(_ path: String) {
        cancelRename()
        deletePath = path
        confirmingDelete = true
    }

    private func beginRename(_ path: String) {
        renamePath = path
        renameDraft.begin(path)
    }

    private func commitRename() {
        guard let path = renamePath else { return }
        if store.renameEntry(path, newName: renameDraft.name) { cancelRename() }
    }

    private func cancelRename() {
        if renamePath != nil { renamePath = nil }
        renameDraft.finish()
    }

    private func isPendingMove(_ path: String) -> Bool {
        guard let target = store.pendingRelocation?.target else { return false }
        return workspacePathIsWithin(value: path, ancestor: target)
    }

    @ViewBuilder private func pendingMoveIndicator(_ path: String) -> some View {
        if store.relocationSlow && isPendingMove(path) {
            ProgressView().controlSize(.mini)
                .accessibilityLabel("Moving… Waiting for confirmation")
                .help("Moving… Waiting for confirmation")
        }
    }

    private func nativeDragSource(_ path: String, directory: Bool) -> some View {
        SidebarNativeDragSource(path: path, directory: directory,
            type: NSPasteboard.PasteboardType(sidebarDragType.identifier),
            enabled: renamePath == nil && !isPendingMove(path), state: sidebarDrag, begin: {
                cancelRename()
                sidebarDrag.begin(path)
            }, end: { }, rowReady: { store.sidebarDropRowReady(path) })
    }

    private func applySidebarRelocation(_ relocation: Relocation?) {
        if let previous = sidebarRelocation, relocation == nil {
            let confirmed = store.directories.contains(previous.target)
                || store.documents.contains { $0.id == previous.target }
            if !confirmed {
                let inverse = Relocation(source: previous.target, target: previous.source)
                expandedDirectories = Set(expandedDirectories.map(inverse.path))
                selectedDirectory = selectedDirectory.map(inverse.path)
            }
        }
        if let relocation, sidebarRelocation != relocation {
            expandedDirectories = Set(expandedDirectories.map(relocation.path))
            selectedDirectory = selectedDirectory.map(relocation.path)
        }
        sidebarRelocation = relocation
        rebuildSidebarIndex()
    }

    private func canDrop(_ directory: String) -> Bool {
        if !sidebarDrag.externalURLs.isEmpty {
            return store.canMoveFiles(sidebarDrag.externalURLs, directory: directory)
        }
        guard let source = sidebarDrag.source else { return false }
        return store.canMoveEntry(source, directory: directory)
    }

    private func nativeDropTarget(_ directory: String) -> some View {
        SidebarNativeDropTarget(type: NSPasteboard.PasteboardType(sidebarDragType.identifier), state: sidebarDrag, canDrop: {
            let valid = canDrop(directory)
            sidebarDrag.updateValidity(valid)
            return valid
        }, entered: {
            sidebarDrag.enter(directory, valid: canDrop(directory), expand: {
                guard sidebarTab == .files, appliedFileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                      canDrop(directory), !directory.isEmpty,
                      !expandedDirectories.contains(directory) else { return false }
                toggleDirectory(directory)
                return true
            }, collapse: {
                if expandedDirectories.contains(directory) { toggleDirectory(directory) }
            })
        }, exited: {
            sidebarDrag.leave(directory)
        }, dropped: {
            guard canDrop(directory), let source = sidebarDrag.source else {
                sidebarDrag.finish()
                store.sidebarDropHandlerFinished(false)
                return false
            }
            sidebarDrag.hidePreview()
            var transaction = Transaction(animation: nil)
            transaction.disablesAnimations = true
            let result = withTransaction(transaction) {
                let accepted = store.moveEntry(source, directory: directory)
                sidebarDrag.finish(committing: accepted ? directory : nil)
                if accepted {
                    if !directory.isEmpty { expandedDirectories.insert(directory) }
                    applySidebarRelocation(store.pendingRelocation)
                }
                return accepted
            }
            store.sidebarDropHandlerFinished(result)
            return result
        }, beginDrop: { store.beginSidebarDrop() }, previewHidden: { store.sidebarDropPreviewHidden() })
        .onDrop(of: [.fileURL], delegate: externalSidebarDrop(directory))
    }

    private func externalSidebarDrop(_ directory: String) -> ExternalFileDrop {
        ExternalFileDrop(entered: { urls in
            guard !urls.isEmpty else { return false }
            sidebarDrag.beginExternal(urls)
            let valid = canDrop(directory)
            sidebarDrag.enter(directory, valid: valid, expand: {
                guard sidebarTab == .files, appliedFileNameQuery.isEmpty,
                      !directory.isEmpty, !expandedDirectories.contains(directory) else { return false }
                toggleDirectory(directory)
                return true
            }, collapse: {
                if expandedDirectories.contains(directory) { toggleDirectory(directory) }
            })
            return valid
        }, exited: { sidebarDrag.leave(directory) }, dropped: { urls in
            guard store.canMoveFiles(urls, directory: directory) else { sidebarDrag.finish(); return false }
            let accepted = store.moveFiles(urls, directory: directory)
            sidebarDrag.finish(committing: accepted ? directory : nil)
            if accepted && !directory.isEmpty { expandedDirectories.insert(directory); rebuildSidebarIndex() }
            return accepted
        })
    }

    private var sidebarHeader: some View {
        VStack(spacing: 0) {
            SidebarTabBar(selection: $sidebarTab)
            .frame(maxWidth: .infinity)
            .frame(height: BonsplitTabBarMetrics.barHeight)
            .onChange(of: sidebarTab) { _, _ in rebuildSidebarIndex() }
            if sidebarTab == .search {
                SearchPanel(store: store)
            }
        }
        .padding(.horizontal, 10)
    }

    var body: some View {
        ZStack {
        if store.workspaceName != nil {
        NavigationSplitView {
            VStack(spacing: 0) {
                // A SwiftUI List diffs its rows in superlinear time: replacing a
                // few thousand rows (filtering, clearing a filter, streamed search
                // results) froze the window for seconds. A lazy stack diffs by ID
                // in linear time and creates only the visible rows.
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(sidebarRows) { row in
                            sidebarRow(row)
                                .padding(.vertical, 3)
                                .frame(minHeight: 24)
                                .padding(.horizontal, 10)
                        }
                        if sidebarTab == .files {
                            // Keep scrollable space after the files, below the floating controls.
                            Color.clear.frame(height: sidebarBottomBarHeight + sidebarRootDropHeight + 8)
                                .allowsHitTesting(false)
                                .accessibilityHidden(true)
                        }
                    }
                    .padding(.vertical, 6)
                }
                .onDrop(of: [.fileURL], delegate: externalSidebarDrop(""))
                .safeAreaBar(edge: .top, spacing: 0) { sidebarHeader }
                .scrollEdgeEffectStyle(.soft, for: .top)
                .onReceive(store.$workspaceRoot) { root in
                    if currentWorkspaceRoot != root {
                        sidebarDrag.finish()
                        cancelRename()
                        currentWorkspaceRoot = root
                        expandedDirectories.removeAll()
                        selectedDirectory = nil
                        rebuildSidebarIndex()
                    }
                }
                .onReceive(store.$sidebarRevision) { _ in
                    DispatchQueue.main.async {
                        rebuildSidebarIndex()
                    }
                }
                .onReceive(store.$pendingRelocation) { relocation in
                    DispatchQueue.main.async { applySidebarRelocation(relocation) }
                }

                .onChange(of: fileNameQuery) { _, query in store.filterFiles(query) }
                .overlay {
                    if sidebarRows.isEmpty && sidebarIndexInProgress {
                        ProgressView("Loading files…")
                    } else if sidebarTab.hasTree && store.workspaceName != nil && sidebarRows.isEmpty && !store.searchInProgress {
                        Text(sidebarTab == .search
                             ? (store.searchQuery.isEmpty ? "Search file contents" : "No text matches")
                             : (appliedFileNameQuery.isEmpty ? "No files" : "No matching names"))
                            .foregroundStyle(.secondary)
                    }
                }
                .overlay(alignment: .bottom) {
                    if sidebarTab == .files {
                    HStack(spacing: 8) {
                        Menu {
                            Button("New File", systemImage: "doc.badge.plus") {
                                creatingDirectory = false
                                newEntryPath = ""
                                creatingEntry = true
                            }
                            Button("New Folder", systemImage: "folder.badge.plus") {
                                creatingDirectory = true
                                newEntryPath = ""
                                creatingEntry = true
                            }
                        } label: {
                            Image(systemName: "plus")
                                .frame(width: 20, height: 20)
                                .padding(8)
                        }
                        .menuIndicator(.hidden)
                        .buttonStyle(.plain)
                        .help("Add file or folder")
                        .glassEffect(.regular, in: Circle())
                        HStack(spacing: 6) {
                            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                            TextField("Filter names", text: $fileNameQuery)
                                .textFieldStyle(.plain)
                            if !fileNameQuery.isEmpty {
                                Button("Clear search", systemImage: "xmark.circle.fill") {
                                    fileNameQuery = ""
                                }
                                .labelStyle(.iconOnly)
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(10)
                        .glassEffect(.regular, in: Capsule())
                    }
                    .padding(.horizontal, 10)
                    .padding(.bottom, 8)
                    .onGeometryChange(for: CGFloat.self) { geometry in
                        geometry.size.height
                    } action: { height in
                        sidebarBottomBarHeight = height
                    }
                    }
                }
                if sidebarTab == .files {
                    SidebarRootDropLabel(drag: sidebarDrag, workspaceName: store.workspaceName ?? "workspace")
                        .overlay(nativeDropTarget(""))
                        .onGeometryChange(for: CGFloat.self) { geometry in
                            geometry.size.height
                        } action: { height in
                            sidebarRootDropHeight = height
                        }
                }
            }
            .navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 480)
        } detail: {
            editorDetail
                .onDrop(of: [.fileURL], delegate: ExternalFileDrop(entered: { !$0.isEmpty },
                    exited: {}, dropped: { store.openFiles($0) }, operation: .copy))
                .toolbar {
                    ToolbarItem(placement: .principal) {
                        Text(store.workspaceName ?? "Native Workspace")
                            .font(.headline)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    .sharedBackgroundVisibility(.hidden)
                    ToolbarSpacer(.flexible, placement: .primaryAction)
                    ToolbarItemGroup(placement: .primaryAction) {
                        Button("Decrease Text Size", systemImage: "textformat.size.smaller") { editorFontSize -= 1 }
                            .disabled(editorFontSize <= 8)
                            .help("Decrease Text Size")
                        Text("\(Int((editorFontSize / NSFont.systemFontSize * 100).rounded()))%")
                            .monospacedDigit()
                            .frame(minWidth: 44)
                            .accessibilityLabel("Text Zoom")
                        Button("Increase Text Size", systemImage: "textformat.size.larger") { editorFontSize += 1 }
                            .disabled(editorFontSize >= 40)
                            .help("Increase Text Size")
                    }
                    ToolbarSpacer(.fixed, placement: .primaryAction)
                    ToolbarItemGroup(placement: .primaryAction) {
                        Button("Save", systemImage: "square.and.arrow.down") { store.saveDocument() }
                            .disabled(!store.canSaveActiveDocument)
                            .accessibilityIdentifier("saveDocument")
                        Button("Save All", systemImage: "square.and.arrow.down.on.square") { store.saveAllDocuments() }
                    }
                    ToolbarSpacer(.fixed, placement: .primaryAction)
                    ToolbarItem(placement: .primaryAction) {
                        ApplicationMenu(store: store, openFolder: { choosingWorkspace = true })
                    }
                }
                .toolbar(removing: .title)
        }
        }
        }
        .disabled(store.editingPaused)
        .onChange(of: store.searchFocusRevision) { _, _ in sidebarTab = .search }
        .confirmationDialog("Delete \"\(deletePath)\"?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button(workspaceDeleteConfirmation(value: deletePath).confirm, role: .destructive) { store.deleteEntry(deletePath) }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text(workspaceDeleteConfirmation(value: deletePath).body)
        }
        .background(WindowTitleView(title: store.workspaceName ?? "Native Workspace"))
        .fileImporter(isPresented: $choosingWorkspace, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url): openWorkspace(url)
            case .failure: break
            }
        }
        .confirmationDialog("This file changed on disk", isPresented: $resolvingExternalChange) {
            Button("Reload from Disk", role: .destructive) { store.reloadDocument() }
            Button("Keep Mine and Overwrite", role: .destructive) { store.overwriteDocument() }
        } message: {
            Text("Choose which version to keep. Reloading discards your local edits.")
        }
        .alert(creatingDirectory ? "New Folder" : "New File", isPresented: $creatingEntry) {
            TextField("Path relative to workspace", text: $newEntryPath)
            Button("Create") {
                store.createEntry(newEntryPath.trimmingCharacters(in: .whitespacesAndNewlines),
                                  directory: creatingDirectory)
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("Enter a name or a path inside an existing folder.")
        }
        .frame(minWidth: 680, minHeight: 420)
    }

    private var editorDetail: some View {
        VStack(spacing: 0) {
            DocumentTabs(documents: store.openDocuments, previewDocument: store.previewDocument,
                         dirtyDocuments: Set(store.documents.filter(\.dirty).map(\.id)),
                         selectedDocument: store.activeDocument?.id,
                         select: { id in
                             store.openDocument(id, preview: true)
                             return store.activeDocument?.id == id
                         }, close: { store.closeDocument($0) }, keepOpen: { store.keepDocumentOpen($0) }) {
                editorPanel
            }
        }
    }

    private var editorPanel: some View {
        VStack(spacing: 0) {
            if let active = store.activeDocument {
                if active.externalChange {
                    HStack(spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                        Text("This file changed on disk")
                        Spacer()
                        Button("Resolve…") { resolvingExternalChange = true }
                    }
                    .font(.callout)
                    .padding(8)
                }
                NoWrapTextEditor(documentID: active.id, documentIdentity: store.editorDocumentIdentity,
                                 textRevision: store.editorContentRevision,
                                 text: Binding(
                    get: { store.editorText },
                    set: { store.changeDocument($0) }
                ), searchRange: store.selectedSearchRange,
                   searchNavigationRevision: store.searchNavigationRevision,
                   fontSize: editorFontSize)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        Image(systemName: "doc.text")
                            .font(.system(size: 40))
                            .foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 8) {
                            Text("No Document Open").font(.title.bold())
                            Text("Choose a file from the sidebar to start editing.")
                                .foregroundStyle(.secondary)
                        }
                        Button("Open Folder…", systemImage: "folder.badge.plus") { choosingWorkspace = true }
                            .keyboardShortcut("o", modifiers: .command)
                            .buttonStyle(.borderedProminent)
                        Divider()
                        Text("Keyboard Shortcuts").font(.headline)
                        Grid(alignment: .leading, horizontalSpacing: 32, verticalSpacing: 12) {
                            shortcutRow("Open Folder", keys: "⌘O")
                            shortcutRow("Save", keys: "⌘S")
                            shortcutRow("Save All", keys: "⇧⌘S")
                        }
                    }
                    .padding(32)
                    .frame(maxWidth: 440, alignment: .leading)
                    .frame(maxWidth: .infinity)
                }
                .defaultScrollAnchor(.center)
                .accessibilityIdentifier("workspaceHub")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            if let error = store.errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .padding(8)
            }
            Divider()
            HStack {
                Text(store.activeDocument?.title ?? "No document")
                Spacer()
                Text(store.editingPaused ? "Working…" : (store.activeDocument == nil ? "Ready" : (store.hasDirtyActiveDocument ? "Modified" : "Saved")))
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
        }
    }

    private func shortcutRow(_ title: String, keys: String) -> some View {
        GridRow {
            Text(title).foregroundStyle(.secondary)
            Text(keys).monospaced().gridColumnAlignment(.trailing)
        }
    }

    private func openWorkspace(_ url: URL) {
        store.openWorkspace(url)
    }
}

private struct WindowTitleView: NSViewRepresentable {
    let title: String

    func makeNSView(context: Context) -> TitleView {
        let view = TitleView()
        view.title = title
        return view
    }

    func updateNSView(_ view: TitleView, context: Context) {
        view.title = title
        view.updateTitle()
    }

    final class TitleView: NSView {
        var title = ""

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            updateTitle()
        }

        func updateTitle() {
            guard let window, window.title != title else { return }
            window.title = title
        }
    }
}
