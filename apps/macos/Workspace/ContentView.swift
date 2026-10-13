import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false
    @State private var showingHome = true
    @State private var resolvingExternalChange = false
    @State private var renamePath: String?
    @State private var renameDraft = SidebarRenameDraft()
    @State private var sidebarDrag = SidebarDragState()
    @State private var creatingEntry = false
    @State private var creatingDirectory = false
    @State private var newEntryPath = ""
    @State private var sidebarTab = 0
    @State private var fileNameQuery = ""
    @State private var sidebarBottomBarHeight: CGFloat = 44
    @State private var sidebarRootDropHeight: CGFloat = 0
    @State private var expandedDirectories: Set<String> = []
    @State private var selectedDirectory: String?
    @State private var currentWorkspaceRoot: String?
    @State private var sidebarChildren: [String: [SidebarOutlineRow]] = [:]
    @State private var sidebarRows: [SidebarOutlineRow] = []
    @State private var sidebarCache: [Int: SidebarIndex] = [:]
    @State private var sidebarRelocation: WorkspaceStore.Relocation?
    private struct SidebarIndex {
        let children: [String: [SidebarOutlineRow]]
        let rows: [SidebarOutlineRow]
    }

    private struct SearchMatchRow {
        let index: Int
        let result: SearchResult
    }

    private struct SearchFileGroup {
        let id: String
        let file: SearchResult
        var matches: [SearchMatchRow] = []
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
        if sidebarTab >= 2 {
            sidebarChildren = [:]
            sidebarRows = []
            return
        }
        if let cached = sidebarCache[sidebarTab] {
            sidebarChildren = cached.children
            sidebarRows = cached.rows
            return
        }
        let textMode = sidebarTab == 1
        let needle = fileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        let searching = textMode || !needle.isEmpty
        var groups: [SearchFileGroup] = []
        var indicesByID: [String: Int] = [:]
        let indexedResults: [(Int, SearchResult)] = textMode
            ? store.sidebarSearchResults.enumerated().compactMap { $0.element.location == nil ? nil : ($0.offset, $0.element) }
            : store.sidebarDocuments.enumerated().compactMap { index, document in
                guard needle.isEmpty || document.id.split(separator: "/").last.map(String.init)?
                    .localizedCaseInsensitiveContains(needle) == true else { return nil }
                return (index, SearchResult(id: document.id, title: document.title,
                                            dirty: document.dirty, snippet: nil, location: nil))
            }
        for (index, result) in indexedResults {
            if let groupIndex = indicesByID[result.id] {
                if result.location != nil {
                    groups[groupIndex].matches.append(SearchMatchRow(index: index, result: result))
                }
            } else {
                indicesByID[result.id] = groups.count
                groups.append(SearchFileGroup(id: result.id, file: result))
                if result.location != nil {
                    groups[groups.count - 1].matches.append(SearchMatchRow(index: index, result: result))
                }
            }
        }

        var directoryPaths: Set<String> = []
        if searching {
            for group in groups {
                let parts = group.id.split(separator: "/")
                for count in 1..<parts.count {
                    directoryPaths.insert(parts.prefix(count).joined(separator: "/"))
                }
            }
            if !textMode {
                for path in store.sidebarDirectories where path.split(separator: "/").last.map(String.init)?
                    .localizedCaseInsensitiveContains(needle) == true {
                    let parts = path.split(separator: "/")
                    for count in 1...parts.count {
                        directoryPaths.insert(parts.prefix(count).joined(separator: "/"))
                    }
                }
            }
        } else {
            directoryPaths = Set(store.sidebarDirectories)
        }

        var children: [String: [SidebarOutlineRow]] = [:]
        func append(_ path: String, kind: SidebarOutlineRow.Kind) {
            let depth = path.split(separator: "/").count - 1
            let parent = path.split(separator: "/").dropLast().joined(separator: "/")
            children[parent, default: []].append(SidebarOutlineRow(id: "entry:\(path)", depth: depth, kind: kind))
        }
        for path in directoryPaths { append(path, kind: .directory(path)) }
        for group in groups { append(group.id, kind: .file(group)) }
        for parent in Array(children.keys) {
            children[parent]?.sort { left, right in
                let leftIsDirectory: Bool
                let rightIsDirectory: Bool
                if case .directory = left.kind { leftIsDirectory = true } else { leftIsDirectory = false }
                if case .directory = right.kind { rightIsDirectory = true } else { rightIsDirectory = false }
                if leftIsDirectory != rightIsDirectory { return leftIsDirectory }
                return left.id.localizedStandardCompare(right.id) == .orderedAscending
            }
        }
        sidebarChildren = children
        refreshSidebarRows()
        sidebarCache[sidebarTab] = SidebarIndex(children: children, rows: sidebarRows)
    }

    private func refreshSidebarRows() {
        sidebarRows = visibleChildren(of: "")
    }

    private func visibleChildren(of parent: String) -> [SidebarOutlineRow] {
        let searching = sidebarTab == 1 || !fileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        var rows: [SidebarOutlineRow] = []
        func appendChildren(of parent: String) {
            for row in sidebarChildren[parent] ?? [] {
                rows.append(row)
                switch row.kind {
                case .directory(let path):
                    if searching || expandedDirectories.contains(path) { appendChildren(of: path) }
                case .file(let group):
                    for match in group.matches {
                        rows.append(SidebarOutlineRow(id: "match:\(match.index)", depth: row.depth + 1, kind: .match(match)))
                    }
                case .match: break
                }
            }
        }
        appendChildren(of: parent)
        return rows
    }

    private func toggleDirectory(_ path: String) {
        let opening = !expandedDirectories.contains(path)
        if opening { expandedDirectories.insert(path) }
        else { expandedDirectories.remove(path) }
        if sidebarTab == 1 || !fileNameQuery.isEmpty {
            sidebarCache.removeValue(forKey: 0)
            return
        }
        guard let index = sidebarRows.firstIndex(where: { $0.id == "entry:\(path)" }) else { return }
        let prefix = path + "/"
        var end = index + 1
        while end < sidebarRows.count {
            let rowPath: String
            switch sidebarRows[end].kind {
            case .directory(let directory): rowPath = directory
            case .file(let file): rowPath = file.id
            case .match(let match): rowPath = match.result.id
            }
            if !rowPath.hasPrefix(prefix) { break }
            end += 1
        }
        sidebarRows.replaceSubrange(index + 1..<end, with: opening ? visibleChildren(of: path) : [])
        sidebarCache[0] = SidebarIndex(children: sidebarChildren, rows: sidebarRows)
    }

    @ViewBuilder private func sidebarRow(_ row: SidebarOutlineRow) -> some View {
        switch row.kind {
        case .directory(let path):
            HStack(spacing: 8) {
                Button { toggleDirectory(path) } label: {
                    Image(systemName: expandedDirectories.contains(path) || sidebarTab == 1 || !fileNameQuery.isEmpty
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
                                         displayPath: store.presentationPath(path), directory: true,
                                         expanded: expandedDirectories.contains(path) || sidebarTab == 1 || !fileNameQuery.isEmpty)
                        entryName(path)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            .padding(.leading, CGFloat(row.depth) * 14 + 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .onDrag { dragProvider(path) } preview: { dragPreview(path, directory: true) }
            .onDrop(of: [sidebarDragType], delegate: entryDropDelegate(path))
            .overlay { SidebarDropOutline(drag: sidebarDrag, directory: path, selected: selectedDirectory == path) }
            .listRowBackground(Color.clear)
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(path); return .handled }
            .contextMenu {
                Button("Rename") { beginRename(path) }
            }
        case .file(let group):
            Button {
                if renamePath != group.id {
                    if selectedDirectory == nil && store.selectedSearchResultIndex == nil
                        && store.activeDocument.map({ store.presentationPath($0.id) }) == group.id {
                        beginRename(group.id)
                    } else {
                        selectedDirectory = nil
                        store.openSearchFile(group.id)
                    }
                }
            } label: {
                HStack(spacing: 8) {
                    SidebarEntryIcon(draft: renameDraft, originalPath: group.id,
                                     displayPath: store.presentationPath(group.id))
                    entryName(group.id)
                    Spacer(minLength: 0)
                    if store.externalChangeIDs.contains(where: { store.presentationPath($0) == group.id }) {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    }
                    if group.file.dirty || (store.activeDocument.map { store.presentationPath($0.id) } == group.id && store.hasDirtyActiveDocument) { Circle().fill(.secondary).frame(width: 6, height: 6) }
                }
                .padding(.leading, CGFloat(row.depth) * 14 + 8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onDrag { dragProvider(group.id) } preview: { dragPreview(group.id, directory: false) }
            .onDrop(of: [sidebarDragType], delegate: entryDropDelegate(parentDirectory(group.id)))
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(group.id); return .handled }
            .contextMenu {
                Button("Rename") { beginRename(group.id) }
            }
            .overlay {
                if selectedDirectory == nil && store.selectedSearchResultIndex == nil
                    && store.activeDocument.map({ store.presentationPath($0.id) }) == group.id {
                    SidebarRowHighlight()
                }
            }
            .listRowBackground(Color.clear)
        case .match(let match):
            Button {
                selectedDirectory = nil
                store.openSearchResult(match.result, at: match.index)
            } label: {
                HStack(alignment: .top, spacing: 8) {
                    if let location = match.result.location {
                        Text("\(location.line):\(location.column)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    }
                    if let snippet = match.result.snippet { Text(snippet).font(.caption).lineLimit(1) }
                    Spacer(minLength: 0)
                }
                .padding(.leading, CGFloat(row.depth) * 14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .overlay {
                if selectedDirectory == nil && store.selectedSearchResultIndex == match.index {
                    SidebarRowHighlight()
                }
            }
            .listRowBackground(Color.clear)
        }
    }

    private let sidebarDragType = UTType(exportedAs: "org.nativeworkspace.sidebar-entry", conformingTo: .data)

    private func parentDirectory(_ path: String) -> String {
        path.split(separator: "/").dropLast().joined(separator: "/")
    }

    @ViewBuilder private func entryName(_ path: String) -> some View {
        if renamePath == path {
            SidebarInlineRename(name: Binding(get: { renameDraft.name }, set: { renameDraft.changeName($0) }),
                                directory: store.directories.contains(path),
                                submit: { commitRename() }, cancel: { cancelRename() })
                .frame(height: 22)
        } else {
            Text(store.presentationPath(path).split(separator: "/").last.map(String.init) ?? path)
        }
    }

    private func beginRename(_ path: String) {
        renamePath = path
        renameDraft.begin(path)
        store.setSidebarRenaming(true)
    }

    private func commitRename() {
        guard let path = renamePath else { return }
        if store.renameEntry(path, newName: renameDraft.name) { cancelRename() }
    }

    private func cancelRename() {
        if renamePath != nil { renamePath = nil }
        renameDraft.finish()
        store.setSidebarRenaming(false)
    }

    private func dragProvider(_ path: String) -> NSItemProvider {
        cancelRename()
        sidebarDrag.begin(path)
        store.beginSidebarDrag()
        let provider = NSItemProvider()
        provider.registerDataRepresentation(forTypeIdentifier: sidebarDragType.identifier, visibility: .ownProcess) {
            completion in completion(Data(path.utf8), nil); return nil
        }
        return provider
    }

    private func dragPreview(_ path: String, directory: Bool) -> some View {
        HStack(spacing: 8) {
            Image(systemName: directory ? "folder.fill" : "doc.fill")
            Text(path.split(separator: "/").last.map(String.init) ?? path)
        }
        .padding(10)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 6))
        .shadow(radius: 5, y: 3)
    }

    private func canDrop(_ directory: String) -> Bool {
        guard let source = sidebarDrag.source else { return false }
        return store.canMoveEntry(source, directory: directory)
    }

    private func entryDropDelegate(_ directory: String) -> SidebarEntryDropDelegate {
        SidebarEntryDropDelegate(type: sidebarDragType, hasActiveDrag: { sidebarDrag.source != nil }, canDrop: {
            let valid = canDrop(directory)
            sidebarDrag.updateValidity(valid)
            return valid
        }, entered: {
            sidebarDrag.enter(directory, valid: canDrop(directory), expand: {
                guard sidebarTab == 0, fileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
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
                store.endSidebarDrag()
                return false
            }
            let accepted = store.moveEntry(source, directory: directory)
            sidebarDrag.finish(committing: accepted ? directory : nil)
            store.endSidebarDrag()
            return accepted
        })
    }

    private var sidebarHeader: some View {
        VStack(spacing: 8) {
            SidebarTabBar(selection: $sidebarTab)
            .frame(maxWidth: .infinity)
            .onChange(of: sidebarTab) { _, _ in rebuildSidebarIndex() }
            if sidebarTab == 1 {
                HStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                    TextField("Search in files", text: Binding(
                        get: { store.searchQuery },
                        set: { store.setSearchQuery($0) }
                    ))
                    .textFieldStyle(.plain)
                    if store.searchInProgress { ProgressView().controlSize(.small) }
                }
                .padding(10)
                .glassEffect(.regular, in: Capsule())
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }

    var body: some View {
        ZStack {
        if store.workspaceName != nil {
        NavigationSplitView {
            VStack(spacing: 0) {
                List {
                    ForEach(sidebarRows) { row in
                        sidebarRow(row)
                    }
                    if sidebarTab == 0 {
                        // Keep scrollable space after the files, below the floating controls.
                        Color.clear.frame(height: sidebarBottomBarHeight + sidebarRootDropHeight + 8)
                            .listRowInsets(EdgeInsets())
                            .listRowSeparator(.hidden)
                            .listRowBackground(Color.clear)
                            .allowsHitTesting(false)
                            .accessibilityHidden(true)
                    }
                }
                .safeAreaBar(edge: .top, spacing: 0) { sidebarHeader }
                .scrollEdgeEffectStyle(.soft, for: .top)
                .onReceive(store.$workspaceRoot) { root in
                    if currentWorkspaceRoot != root {
                        sidebarDrag.finish()
                        store.endSidebarDrag()
                        cancelRename()
                        currentWorkspaceRoot = root
                        expandedDirectories.removeAll()
                        selectedDirectory = nil
                        sidebarCache.removeAll()
                        rebuildSidebarIndex()
                    }
                }
                .onReceive(store.$sidebarRevision) { _ in
                    DispatchQueue.main.async {
                        sidebarCache.removeAll()
                        rebuildSidebarIndex()
                    }
                }
                .onReceive(store.$pendingRelocation) { relocation in
                    DispatchQueue.main.async {
                        if let previous = sidebarRelocation, relocation == nil {
                            let confirmed = store.directories.contains(previous.target)
                                || store.documents.contains { $0.id == previous.target }
                            if !confirmed {
                                let inverse = WorkspaceStore.Relocation(source: previous.target, target: previous.source)
                                expandedDirectories = Set(expandedDirectories.map(inverse.path))
                                selectedDirectory = selectedDirectory.map(inverse.path)
                            }
                        }
                        if let relocation, sidebarRelocation != relocation {
                            expandedDirectories = Set(expandedDirectories.map(relocation.path))
                            selectedDirectory = selectedDirectory.map(relocation.path)
                        }
                        sidebarRelocation = relocation
                        sidebarCache.removeAll()
                        rebuildSidebarIndex()
                    }
                }
                .onChange(of: fileNameQuery) { _, _ in
                    sidebarCache.removeValue(forKey: 0)
                    rebuildSidebarIndex()
                }
                .overlay {
                    if sidebarTab < 2 && store.workspaceName != nil && sidebarRows.isEmpty && !store.searchInProgress {
                        Text(sidebarTab == 1
                             ? (store.searchQuery.isEmpty ? "Search file contents" : "No text matches")
                             : (fileNameQuery.isEmpty ? "No files" : "No matching names"))
                            .foregroundStyle(.secondary)
                    }
                }
                .overlay(alignment: .bottom) {
                    if sidebarTab == 0 {
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
                if sidebarTab == 0 {
                    SidebarRootDropLabel(drag: sidebarDrag, workspaceName: store.workspaceName ?? "workspace")
                        .onDrop(of: [sidebarDragType], delegate: entryDropDelegate(""))
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
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Home", systemImage: "house") { showingHome = true }
                    }
                }
        }
        .opacity(showingHome ? 0 : 1)
        .allowsHitTesting(!showingHome)
        }
        if showingHome || store.workspaceName == nil {
            VStack(alignment: .leading, spacing: 24) {
                Label("Native Workspace", systemImage: "folder")
                    .font(.largeTitle.bold())
                if store.diagnosticsEnabled {
                    Button("Developer Tools", systemImage: "waveform.path.ecg") { store.openDevtools() }
                }
                Button("Open Folder…", systemImage: "folder.badge.plus") {
                    choosingWorkspace = true
                }
                .buttonStyle(.borderedProminent)
                Text("Recent Workspaces").font(.title2.bold())
                if store.recentWorkspaces.isEmpty {
                    Text("No recent workspaces").foregroundStyle(.secondary)
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 4) {
                            ForEach(store.recentWorkspaces, id: \.root) { workspace in
                                Button {
                                    openWorkspace(URL(fileURLWithPath: workspace.root, isDirectory: true))
                                } label: {
                                    Label {
                                        VStack(alignment: .leading) {
                                            Text(workspace.name)
                                            Text(workspace.root).font(.caption).foregroundStyle(.secondary)
                                        }
                                    } icon: { Image(systemName: "folder") }
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                .padding(8)
                            }
                        }
                    }
                }
                Spacer(minLength: 0)
                if let error = store.errorMessage {
                    Text(error).foregroundStyle(.red)
                }
            }
            .padding(32)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
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
        .onReceive(Timer.publish(every: 1, on: .main, in: .common).autoconnect()) { _ in
            store.pollWorkspaceChanges()
        }
        .onReceive(Timer.publish(every: 0.25, on: .main, in: .common).autoconnect()) { _ in
            store.pollSearchResults()
        }
        .frame(minWidth: 680, minHeight: 420)
    }

    private var editorDetail: some View {
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
                       searchNavigationRevision: store.searchNavigationRevision)
                } else {
                    VStack(spacing: 16) {
                        ContentUnavailableView(
                            store.workspaceName == nil ? "Open a Workspace" : "Select a Document",
                            systemImage: store.workspaceName == nil ? "folder" : "doc.text",
                            description: Text(store.workspaceName == nil ? "Choose a directory containing Markdown or text files." : "Choose a file in the sidebar.")
                        )
                        if store.workspaceName == nil {
                            Button("Open Workspace") { choosingWorkspace = true }
                                .buttonStyle(.glassProminent)
                        }
                    }
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
                    Text(store.activeDocument == nil ? "Ready" : (store.hasDirtyActiveDocument ? "Modified" : "Saved"))
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
            }
    }

    private func openWorkspace(_ url: URL) {
        store.openWorkspace(url)
        if store.workspaceRoot == url.path { showingHome = false }
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

private struct SidebarEntryDropDelegate: DropDelegate {
    let type: UTType
    let hasActiveDrag: () -> Bool
    let canDrop: () -> Bool
    let entered: () -> Void
    let exited: () -> Void
    let dropped: () -> Bool

    func validateDrop(info: DropInfo) -> Bool {
        // suggestedName is optional and is not preserved by every native
        // pasteboard bridge. The own-process type and live session identify us.
        hasActiveDrag() && info.hasItemsConforming(to: [type])
    }
    func dropEntered(info: DropInfo) { entered() }
    func dropExited(info: DropInfo) { exited() }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        entered()
        return DropProposal(operation: canDrop() ? .move : .forbidden)
    }
    func performDrop(info: DropInfo) -> Bool { validateDrop(info: info) && dropped() }
}
