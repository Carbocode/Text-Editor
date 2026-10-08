import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false
    @State private var resolvingExternalChange = false
    @State private var expandedDirectories: Set<String> = []
    @State private var currentWorkspaceRoot: String?
    @State private var sidebarChildren: [String: [SidebarOutlineRow]] = [:]
    @State private var sidebarRows: [SidebarOutlineRow] = []

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
        let searching = !store.searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        var groups: [SearchFileGroup] = []
        var indicesByID: [String: Int] = [:]
        for (index, result) in store.searchResults.enumerated() {
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
    }

    private func refreshSidebarRows() {
        sidebarRows = visibleChildren(of: "")
    }

    private func visibleChildren(of parent: String) -> [SidebarOutlineRow] {
        let searching = !store.searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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
        if !store.searchQuery.isEmpty { return }
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
    }

    @ViewBuilder private func sidebarRow(_ row: SidebarOutlineRow) -> some View {
        switch row.kind {
        case .directory(let path):
            Button {
                toggleDirectory(path)
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: expandedDirectories.contains(path) || !store.searchQuery.isEmpty
                        ? "chevron.down" : "chevron.right").font(.caption)
                    if let icon = WorkspaceIcons.image(for: path, isDirectory: true,
                                                      expanded: expandedDirectories.contains(path) || !store.searchQuery.isEmpty) {
                        Image(nsImage: icon).resizable().interpolation(.high).frame(width: 16, height: 16)
                    }
                    Text(path.split(separator: "/").last.map(String.init) ?? path)
                }
            }
            .buttonStyle(.plain)
        case .file(let group):
            Button {
                store.openSearchFile(group.id)
            } label: {
                HStack(spacing: 8) {
                    if let icon = WorkspaceIcons.image(for: group.id) {
                        Image(nsImage: icon).resizable().interpolation(.high).frame(width: 16, height: 16)
                    }
                    Text(group.id.split(separator: "/").last.map(String.init) ?? group.id)
                    Spacer(minLength: 0)
                    if store.externalChangeIDs.contains(group.id) {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    }
                    if group.file.dirty { Circle().fill(.secondary).frame(width: 6, height: 6) }
                }
            }
            .buttonStyle(.plain)
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
            }
            .buttonStyle(.plain)
            .listRowBackground(store.selectedSearchResultIndex == match.index
                ? Color.accentColor.opacity(0.12) : Color.clear)
        }
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("Search files", text: Binding(
                        get: { store.searchQuery },
                        set: { store.setSearchQuery($0) }
                    ))
                    .textFieldStyle(.plain)
                    if store.searchInProgress { ProgressView().controlSize(.small) }
                    if !store.searchQuery.isEmpty {
                        Button("Clear search", systemImage: "xmark.circle.fill") {
                            store.setSearchQuery("")
                        }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.plain)
                    }
                }
                .padding(8)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
                .padding(.horizontal, 10)
                .padding(.top, 8)
                .disabled(store.workspaceName == nil)
                List {
                    ForEach(sidebarRows) { row in
                        sidebarRow(row).padding(.leading, CGFloat(row.depth) * 14)
                    }
                }
                .onReceive(store.$workspaceRoot) { root in
                    if currentWorkspaceRoot != root {
                        currentWorkspaceRoot = root
                        expandedDirectories.removeAll()
                    }
                }
                .onReceive(store.$sidebarRevision) { _ in
                    DispatchQueue.main.async { rebuildSidebarIndex() }
                }
                .overlay {
                    if store.workspaceName != nil && store.searchResults.isEmpty
                        && (store.searchQuery.isEmpty ? store.directories.isEmpty : !store.searchInProgress) {
                        Text(store.searchQuery.isEmpty ? "No files" : "No matching files")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle(store.workspaceName ?? "Native Workspace")
            .toolbar {
                Button("Open Workspace", systemImage: "folder") {
                    choosingWorkspace = true
                }
                Menu {
                    ForEach(store.recentWorkspaces, id: \.root) { workspace in
                        Button(workspace.name) {
                            store.openWorkspace(URL(fileURLWithPath: workspace.root, isDirectory: true))
                        }
                        .help(workspace.root)
                    }
                } label: {
                    Label("Recent Workspaces", systemImage: "clock.arrow.circlepath")
                }
                .disabled(store.recentWorkspaces.isEmpty)
            }
        } detail: {
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
                    NoWrapTextEditor(documentID: active.id, text: Binding(
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
            .toolbar {
                Button("Save", systemImage: "square.and.arrow.down") {
                    store.saveDocument()
                }
                .disabled(!store.hasDirtyActiveDocument)
                .keyboardShortcut("s", modifiers: .command)
            }
        }
        .fileImporter(isPresented: $choosingWorkspace, allowedContentTypes: [.folder]) { result in
            switch result {
            case .success(let url): store.openWorkspace(url)
            case .failure: break
            }
        }
        .confirmationDialog("This file changed on disk", isPresented: $resolvingExternalChange) {
            Button("Reload from Disk", role: .destructive) { store.reloadDocument() }
            Button("Keep Mine and Overwrite", role: .destructive) { store.overwriteDocument() }
        } message: {
            Text("Choose which version to keep. Reloading discards your local edits.")
        }
        .onReceive(Timer.publish(every: 0.25, on: .main, in: .common).autoconnect()) { _ in
            store.pollWorkspaceChanges()
        }
        .frame(minWidth: 680, minHeight: 420)
    }
}
