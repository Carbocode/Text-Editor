import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false
    @State private var showingHome = true
    @State private var resolvingExternalChange = false
    @State private var renamePath: String?
    @State private var renameName = ""
    @State private var draggedPath: String?
    @State private var dropDirectory: String?
    @State private var hoverGeneration = UUID()
    @State private var creatingEntry = false
    @State private var creatingDirectory = false
    @State private var newEntryPath = ""
    @State private var sidebarTab = 0
    @State private var fileNameQuery = ""
    @State private var expandedDirectories: Set<String> = []
    @State private var currentWorkspaceRoot: String?
    @State private var sidebarChildren: [String: [SidebarOutlineRow]] = [:]
    @State private var sidebarRows: [SidebarOutlineRow] = []
    @State private var sidebarCache: [Int: SidebarIndex] = [:]
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
            ? store.searchResults.enumerated().compactMap { $0.element.location == nil ? nil : ($0.offset, $0.element) }
            : store.documents.enumerated().compactMap { index, document in
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
                for path in store.directories where path.split(separator: "/").last.map(String.init)?
                    .localizedCaseInsensitiveContains(needle) == true {
                    let parts = path.split(separator: "/")
                    for count in 1...parts.count {
                        directoryPaths.insert(parts.prefix(count).joined(separator: "/"))
                    }
                }
            }
        } else {
            directoryPaths = Set(store.directories)
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
        if sidebarTab == 1 || !fileNameQuery.isEmpty { return }
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
            Button {
                if renamePath != path { toggleDirectory(path) }
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: expandedDirectories.contains(path) || sidebarTab == 1 || !fileNameQuery.isEmpty
                        ? "chevron.down" : "chevron.right").font(.caption)
                    if let icon = WorkspaceIcons.image(for: path, isDirectory: true,
                                                      expanded: expandedDirectories.contains(path) || sidebarTab == 1 || !fileNameQuery.isEmpty) {
                        Image(nsImage: icon).resizable().interpolation(.high).frame(width: 16, height: 16)
                    }
                    entryName(path)
                }
                .padding(.leading, CGFloat(row.depth) * 14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onDrag { dragProvider(path) } preview: { dragPreview(path, directory: true) }
            .onDrop(of: [sidebarDragType], delegate: entryDropDelegate(path))
            .overlay { dropOutline(path) }
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(path); return .handled }
            .contextMenu {
                Button("Rename") { beginRename(path) }
            }
        case .file(let group):
            Button {
                if renamePath != group.id { store.openSearchFile(group.id) }
            } label: {
                HStack(spacing: 8) {
                    if let icon = WorkspaceIcons.image(for: group.id) {
                        Image(nsImage: icon).resizable().interpolation(.high).frame(width: 16, height: 16)
                    }
                    entryName(group.id)
                    Spacer(minLength: 0)
                    if store.externalChangeIDs.contains(group.id) {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    }
                    if group.file.dirty { Circle().fill(.secondary).frame(width: 6, height: 6) }
                }
                .padding(.leading, CGFloat(row.depth) * 14)
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
            .listRowBackground(store.selectedSearchResultIndex == nil && store.activeDocument?.id == group.id
                ? Color.accentColor.opacity(0.12) : Color.clear)
        case .match(let match):
            Button {
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
            .listRowBackground(store.selectedSearchResultIndex == match.index
                ? Color.accentColor.opacity(0.12) : Color.clear)
        }
    }

    private let sidebarDragType = UTType(exportedAs: "org.nativeworkspace.sidebar-entry")

    private func parentDirectory(_ path: String) -> String {
        path.split(separator: "/").dropLast().joined(separator: "/")
    }

    @ViewBuilder private func entryName(_ path: String) -> some View {
        if renamePath == path {
            SidebarInlineRename(name: $renameName, directory: store.directories.contains(path),
                                submit: { commitRename() }, cancel: { renamePath = nil })
                .frame(height: 22)
        } else {
            Text(path.split(separator: "/").last.map(String.init) ?? path)
        }
    }

    private func beginRename(_ path: String) {
        renamePath = path
        renameName = path.split(separator: "/").last.map(String.init) ?? path
    }

    private func commitRename() {
        guard let path = renamePath else { return }
        store.renameEntry(path, newName: renameName)
        if store.errorMessage == nil { renamePath = nil }
    }

    private func dragProvider(_ path: String) -> NSItemProvider {
        renamePath = nil
        draggedPath = path
        dropDirectory = nil
        hoverGeneration = UUID()
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
        guard let source = draggedPath else { return false }
        let name = source.split(separator: "/").last.map(String.init) ?? source
        let target = directory.isEmpty ? name : directory + "/" + name
        return target != source && !target.hasPrefix(source + "/")
            && !store.documents.contains(where: { $0.id == target || $0.id.hasPrefix(target + "/") })
            && !store.directories.contains(target)
    }

    @ViewBuilder private func dropOutline(_ directory: String) -> some View {
        if dropDirectory == directory {
            RoundedRectangle(cornerRadius: 4)
                .stroke(canDrop(directory) ? Color.accentColor : Color.red, lineWidth: 2)
                .allowsHitTesting(false)
        }
    }

    private func entryDropDelegate(_ directory: String) -> SidebarEntryDropDelegate {
        SidebarEntryDropDelegate(type: sidebarDragType, canDrop: { canDrop(directory) }, entered: {
            dropDirectory = directory
            let generation = UUID()
            hoverGeneration = generation
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.65) {
                guard hoverGeneration == generation, dropDirectory == directory,
                      canDrop(directory), !directory.isEmpty,
                      !expandedDirectories.contains(directory) else { return }
                toggleDirectory(directory)
            }
        }, exited: {
            if dropDirectory == directory { dropDirectory = nil; hoverGeneration = UUID() }
        }, dropped: {
            guard canDrop(directory), let source = draggedPath else { return false }
            store.moveEntry(source, directory: directory)
            dropDirectory = nil
            draggedPath = nil
            hoverGeneration = UUID()
            return store.errorMessage == nil
        })
    }

    var body: some View {
        ZStack {
        if store.workspaceName != nil {
        NavigationSplitView {
            VStack(spacing: 0) {
                List {
                    Color.clear.frame(height: sidebarTab == 1 ? 112 : 58)
                        .listRowSeparator(.hidden)
                    ForEach(sidebarRows) { row in
                        sidebarRow(row)
                    }
                    if sidebarTab == 0 {
                        Text(dropDirectory == "" ? "Move to \(store.workspaceName ?? "workspace")" : "")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 52)
                            .contentShape(Rectangle())
                            .onDrop(of: [sidebarDragType], delegate: entryDropDelegate(""))
                            .overlay { dropOutline("") }
                            .listRowSeparator(.hidden)
                    }
                }
                .onReceive(store.$workspaceRoot) { root in
                    if currentWorkspaceRoot != root {
                        currentWorkspaceRoot = root
                        expandedDirectories.removeAll()
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
                .overlay(alignment: .top) {
                    VStack(spacing: 8) {
                        Picker("Sidebar", selection: $sidebarTab) {
                            Image(systemName: "folder").accessibilityLabel("Files").tag(0)
                            Image(systemName: "magnifyingglass").accessibilityLabel("Search").tag(1)
                            Image(systemName: "arrow.triangle.branch").accessibilityLabel("Git").tag(2)
                            Image(systemName: "bookmark").accessibilityLabel("Bookmarks").tag(3)
                            Image(systemName: "exclamationmark.triangle").accessibilityLabel("Warnings").tag(4)
                            Image(systemName: "xmark.octagon").accessibilityLabel("Errors").tag(5)
                        }
                        .pickerStyle(.segmented)
                        .controlSize(.large)
                        .labelsHidden()
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
                    .padding(.top, 8)
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
                    }
                }
            }
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
                    NoWrapTextEditor(documentID: active.id,
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
    let canDrop: () -> Bool
    let entered: () -> Void
    let exited: () -> Void
    let dropped: () -> Bool

    func validateDrop(info: DropInfo) -> Bool { info.hasItemsConforming(to: [type]) }
    func dropEntered(info: DropInfo) { entered() }
    func dropExited(info: DropInfo) { exited() }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: canDrop() ? .move : .forbidden)
    }
    func performDrop(info: DropInfo) -> Bool { dropped() }
}
