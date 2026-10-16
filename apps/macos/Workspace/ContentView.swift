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
    @State private var sidebarTab = 0
    @State private var fileNameQuery = ""
    /// The filter the sidebar shows; follows `fileNameQuery` once typing pauses.
    @State private var appliedFileNameQuery = ""
    @State private var fileNameDebounce: Task<Void, Never>?
    @State private var sidebarBottomBarHeight: CGFloat = 44
    @State private var sidebarRootDropHeight: CGFloat = 0
    @State private var expandedDirectories: Set<String> = []
    @State private var selectedDirectory: String?
    @State private var currentWorkspaceRoot: String?
    @State private var sidebarChildren: [String: [SidebarOutlineRow]] = [:]
    @State private var sidebarRows: [SidebarOutlineRow] = []
    @State private var sidebarCache: [Int: SidebarIndex] = [:]
    @State private var sidebarIndexGeneration = UUID()
    @State private var sidebarIndexInProgress = false
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
        sidebarIndexGeneration = UUID()
        if sidebarTab >= 2 {
            sidebarIndexInProgress = false
            sidebarChildren = [:]
            sidebarRows = []
            return
        }
        if let cached = sidebarCache[sidebarTab] {
            sidebarIndexInProgress = false
            sidebarChildren = cached.children
            sidebarRows = cached.rows
            return
        }
        sidebarIndexInProgress = true
        let generation = UUID()
        sidebarIndexGeneration = generation
        let tab = sidebarTab
        let query = appliedFileNameQuery
        let documents = store.documents
        let directories = store.directories
        let searchResults = store.searchResults
        let relocation = store.pendingRelocation
        DispatchQueue.global(qos: .userInitiated).async {
            let children = Self.buildSidebarChildren(documents: documents, directories: directories,
                searchResults: searchResults, relocation: relocation, tab: tab, query: query)
            DispatchQueue.main.async {
                guard sidebarIndexGeneration == generation, sidebarTab == tab else { return }
                sidebarIndexInProgress = false
                sidebarChildren = children
                refreshSidebarRows()
                sidebarCache[tab] = SidebarIndex(children: children, rows: sidebarRows)
            }
        }
    }

    nonisolated private static func buildSidebarChildren(documents: [DocumentSummary], directories: [String],
        searchResults: [SearchResult], relocation: WorkspaceStore.Relocation?, tab: Int, query: String)
        -> [String: [SidebarOutlineRow]] {
        let documents = documents.map { document in
            var displayed = document
            displayed.id = relocation?.path(document.id) ?? document.id
            displayed.title = displayed.id.split(separator: "/").last.map(String.init) ?? displayed.id
            return displayed
        }
        let directories = directories.map { relocation?.path($0) ?? $0 }
        let searchResults = searchResults.map { result in
            var displayed = result
            displayed.id = relocation?.path(result.id) ?? result.id
            return displayed
        }
        let textMode = tab == 1
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let searching = textMode || !needle.isEmpty
        var groups: [SearchFileGroup] = []
        var indicesByID: [String: Int] = [:]
        let indexedResults: [(Int, SearchResult)] = textMode
            ? searchResults.enumerated().compactMap { $0.element.location == nil ? nil : ($0.offset, $0.element) }
            : documents.enumerated().compactMap { index, document in
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
                for path in directories where path.split(separator: "/").last.map(String.init)?
                    .localizedCaseInsensitiveContains(needle) == true {
                    let parts = path.split(separator: "/")
                    for count in 1...parts.count {
                        directoryPaths.insert(parts.prefix(count).joined(separator: "/"))
                    }
                }
            }
        } else {
            directoryPaths = Set(directories)
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
        return children
    }

    private func refreshSidebarRows() {
        sidebarRows = visibleChildren(of: "")
    }

    private func visibleChildren(of parent: String) -> [SidebarOutlineRow] {
        let searching = sidebarTab == 1 || !appliedFileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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
        if sidebarTab == 1 || !appliedFileNameQuery.isEmpty {
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
                    Image(systemName: expandedDirectories.contains(path) || sidebarTab == 1 || !appliedFileNameQuery.isEmpty
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
                                         expanded: expandedDirectories.contains(path) || sidebarTab == 1 || !appliedFileNameQuery.isEmpty)
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
                    entryName(group.id).opacity(isPendingMove(group.id) ? 0.5 : 1)
                    Spacer(minLength: 0)
                    pendingMoveIndicator(group.id)
                    if store.openDocuments.contains(where: { store.presentationPath($0) == group.id }) {
                        Image(systemName: "eye")
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Open in editor")
                            .accessibilityIdentifier("sidebarOpen:" + group.id)
                            .help("Open in editor")
                    }
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
            .background(nativeDragSource(group.id, directory: false))
            .overlay(nativeDropTarget(parentDirectory(group.id)))
            .onKeyPress(KeyEquivalent("\u{F705}")) { beginRename(group.id); return .handled }
            .contextMenu {
                Button("Rename") { beginRename(group.id) }
                Button("Delete…", role: .destructive) { requestDelete(group.id) }
            }
            .overlay {
                if selectedDirectory == nil && store.selectedSearchResultIndex == nil
                    && store.activeDocument.map({ store.presentationPath($0.id) }) == group.id {
                    SidebarRowHighlight()
                }
            }
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

    private func requestDelete(_ path: String) {
        cancelRename()
        deletePath = path
        confirmingDelete = true
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

    private func isPendingMove(_ path: String) -> Bool {
        guard let target = store.pendingRelocation?.target else { return false }
        return path == target || path.hasPrefix(target + "/")
    }

    @ViewBuilder private func pendingMoveIndicator(_ path: String) -> some View {
        if isPendingMove(path) {
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
                store.beginSidebarDrag()
            }, end: { store.endSidebarDrag() }, rowReady: { store.sidebarDropRowReady(path) })
    }

    private func relocateSidebarChildren(_ relocation: WorkspaceStore.Relocation) {
        func remap(_ row: SidebarOutlineRow) -> SidebarOutlineRow {
            let kind: SidebarOutlineRow.Kind
            let path: String
            switch row.kind {
            case .directory(let original):
                path = relocation.path(original)
                kind = .directory(path)
            case .file(var group):
                path = relocation.path(group.id)
                var file = group.file
                file.id = path
                file.title = path.split(separator: "/").last.map(String.init) ?? path
                let matches = group.matches.map { match in
                    var result = match.result
                    result.id = path
                    result.title = file.title
                    return SearchMatchRow(index: match.index, result: result)
                }
                group = SearchFileGroup(id: path, file: file, matches: matches)
                kind = .file(group)
            case .match: return row
            }
            return SidebarOutlineRow(id: "entry:\(path)", depth: path.split(separator: "/").count - 1, kind: kind)
        }
        let sourceParent = parentDirectory(relocation.source)
        let targetParent = parentDirectory(relocation.target)
        guard let root = sidebarChildren[sourceParent]?.first(where: { $0.id == "entry:\(relocation.source)" }) else { return }
        sidebarChildren[sourceParent]?.removeAll { $0.id == root.id }
        for key in Array(sidebarChildren.keys) where key == relocation.source || key.hasPrefix(relocation.source + "/") {
            let rows = sidebarChildren.removeValue(forKey: key) ?? []
            sidebarChildren[relocation.path(key)] = rows.map(remap)
        }
        let moved = remap(root)
        var siblings = sidebarChildren[targetParent] ?? []
        let index = siblings.firstIndex { row in
            let rowDirectory: Bool
            let movedDirectory: Bool
            if case .directory = row.kind { rowDirectory = true } else { rowDirectory = false }
            if case .directory = moved.kind { movedDirectory = true } else { movedDirectory = false }
            if rowDirectory != movedDirectory { return movedDirectory }
            return moved.id.localizedStandardCompare(row.id) == .orderedAscending
        } ?? siblings.count
        siblings.insert(moved, at: index)
        sidebarChildren[targetParent] = siblings
        refreshSidebarRows()
    }

    private func applySidebarRelocation(_ relocation: WorkspaceStore.Relocation?) {
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
        if let relocation, sidebarRelocation != relocation {
            relocateSidebarChildren(relocation)
        } else if let previous = sidebarRelocation, relocation == nil,
                  !store.directories.contains(previous.target),
                  !store.documents.contains(where: { $0.id == previous.target }) {
            relocateSidebarChildren(WorkspaceStore.Relocation(source: previous.target, target: previous.source))
        }
        sidebarRelocation = relocation
        sidebarCache.removeAll()
        rebuildSidebarIndex()
    }

    private func canDrop(_ directory: String) -> Bool {
        guard let source = sidebarDrag.source else { return false }
        return store.canMoveEntry(source, directory: directory)
    }

    private func nativeDropTarget(_ directory: String) -> SidebarNativeDropTarget {
        SidebarNativeDropTarget(type: NSPasteboard.PasteboardType(sidebarDragType.identifier), state: sidebarDrag, canDrop: {
            let valid = canDrop(directory)
            sidebarDrag.updateValidity(valid)
            return valid
        }, entered: {
            sidebarDrag.enter(directory, valid: canDrop(directory), expand: {
                guard sidebarTab == 0, appliedFileNameQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
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
                store.sidebarDropHandlerFinished(false)
                return false
            }
            sidebarDrag.hidePreview()
            var transaction = Transaction(animation: nil)
            transaction.disablesAnimations = true
            let result = withTransaction(transaction) {
                let accepted = store.moveEntry(source, directory: directory)
                sidebarDrag.finish(committing: accepted ? directory : nil)
                store.endSidebarDrag()
                if accepted {
                    if !directory.isEmpty { expandedDirectories.insert(directory) }
                    applySidebarRelocation(store.pendingRelocation)
                }
                return accepted
            }
            store.sidebarDropHandlerFinished(result)
            return result
        }, beginDrop: { store.beginSidebarDrop() }, previewHidden: { store.sidebarDropPreviewHidden() })
    }

    private var sidebarHeader: some View {
        VStack(spacing: 0) {
            SidebarTabBar(selection: $sidebarTab)
            .frame(maxWidth: .infinity)
            .frame(height: BonsplitTabBarMetrics.barHeight)
            .onChange(of: sidebarTab) { _, _ in rebuildSidebarIndex() }
            if sidebarTab == 1 {
                HStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                    TextField("Search in files", text: Binding(
                        get: { store.searchFieldText },
                        set: { store.setSearchQuery($0) }
                    ))
                    .textFieldStyle(.plain)
                    if store.searchInProgress { ProgressView().controlSize(.small) }
                }
                .padding(10)
                .glassEffect(.regular, in: Capsule())
                .padding(.bottom, 8)
                if store.searchTruncated {
                    Text("Showing the first 20,000 matches. Refine the search to see the rest.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
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
                        if sidebarTab == 0 {
                            // Keep scrollable space after the files, below the floating controls.
                            Color.clear.frame(height: sidebarBottomBarHeight + sidebarRootDropHeight + 8)
                                .allowsHitTesting(false)
                                .accessibilityHidden(true)
                        }
                    }
                    .padding(.vertical, 6)
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
                    DispatchQueue.main.async { applySidebarRelocation(relocation) }
                }

                .onChange(of: fileNameQuery) { _, query in
                    fileNameDebounce?.cancel()
                    // Clearing applies at once; typing waits for a pause instead of
                    // reindexing the whole workspace on every keystroke.
                    if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        appliedFileNameQuery = query
                        return
                    }
                    fileNameDebounce = Task { @MainActor in
                        try? await Task.sleep(for: .milliseconds(150))
                        guard !Task.isCancelled else { return }
                        appliedFileNameQuery = query
                    }
                }
                .onChange(of: appliedFileNameQuery) { _, _ in
                    sidebarCache.removeValue(forKey: 0)
                    rebuildSidebarIndex()
                }
                .overlay {
                    if sidebarRows.isEmpty && sidebarIndexInProgress {
                        ProgressView("Loading files…")
                    } else if sidebarTab < 2 && store.workspaceName != nil && sidebarRows.isEmpty && !store.searchInProgress {
                        Text(sidebarTab == 1
                             ? (store.searchQuery.isEmpty ? "Search file contents" : "No text matches")
                             : (appliedFileNameQuery.isEmpty ? "No files" : "No matching names"))
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
                            .disabled(!store.hasDirtyActiveDocument)
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
        .disabled(store.deletionPending)
        .confirmationDialog("Delete \"\(deletePath)\"?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button("Delete Permanently", role: .destructive) { store.deleteEntry(deletePath) }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("This permanently deletes the entry and all its contents. Modified documents must be saved first. This cannot be undone.")
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
        // Matches stream in while a search runs; the poll is a no-op otherwise.
        .onReceive(Timer.publish(every: 0.1, on: .main, in: .common).autoconnect()) { _ in
            store.pollSearchResults()
        }
        .frame(minWidth: 680, minHeight: 420)
    }

    private var editorDetail: some View {
        VStack(spacing: 0) {
            DocumentTabs(documents: store.openDocuments,
                         dirtyDocuments: Set(store.documents.filter(\.dirty).map(\.id)),
                         selectedDocument: store.activeDocument?.id,
                         select: { id in
                             store.openDocument(id)
                             return store.activeDocument?.id == id
                         }, close: { store.closeDocument($0) }) {
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
                Text(store.deletionPending ? "Deleting…" : (store.activeDocument == nil ? "Ready" : (store.hasDirtyActiveDocument ? "Modified" : "Saved")))
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
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
