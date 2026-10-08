import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false
    @State private var resolvingExternalChange = false

    private struct SearchMatchRow {
        let index: Int
        let result: SearchResult
    }

    private struct SearchFileGroup {
        let id: String
        let file: SearchResult
        var matches: [SearchMatchRow] = []
    }

    private final class SidebarTreeNode: Identifiable {
        enum Kind {
            case directory(String)
            case file(SearchFileGroup)
            case match(SearchMatchRow)
        }

        let id: String
        let kind: Kind
        var children: [SidebarTreeNode]?

        init(id: String, kind: Kind, children: [SidebarTreeNode]? = nil) {
            self.id = id
            self.kind = kind
            self.children = children
        }

        var isDirectory: Bool {
            if case .directory = kind { return true }
            return false
        }
    }

    private var searchFileGroups: [SearchFileGroup] {
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
        return groups
    }

    private var sidebarTree: [SidebarTreeNode] {
        let root = SidebarTreeNode(id: "root", kind: .directory(""), children: [])
        var directoryNodes: [String: SidebarTreeNode] = ["": root]

        func ensureDirectory(_ path: String) -> SidebarTreeNode {
            if let existing = directoryNodes[path] { return existing }
            let parent = path.split(separator: "/").dropLast().joined(separator: "/")
            let node = SidebarTreeNode(id: "directory:\(path)", kind: .directory(path), children: [])
            ensureDirectory(parent).children?.append(node)
            directoryNodes[path] = node
            return node
        }

        if store.searchQuery.isEmpty {
            for path in store.directories { _ = ensureDirectory(path) }
        }
        for group in searchFileGroups {
            let parent = group.id.split(separator: "/").dropLast().joined(separator: "/")
            let matches = group.matches.map { SidebarTreeNode(id: "match:\($0.index)", kind: .match($0)) }
            let node = SidebarTreeNode(id: "file:\(group.id)", kind: .file(group), children: matches.isEmpty ? nil : matches)
            ensureDirectory(parent).children?.append(node)
        }

        func sort(_ node: SidebarTreeNode) {
            node.children?.sort {
                if $0.isDirectory != $1.isDirectory { return $0.isDirectory }
                return $0.id.localizedStandardCompare($1.id) == .orderedAscending
            }
            node.children?.forEach(sort)
        }
        sort(root)
        return root.children ?? []
    }

    @ViewBuilder private func sidebarRow(_ node: SidebarTreeNode) -> some View {
        switch node.kind {
        case .directory(let path):
            Label(path.split(separator: "/").last.map(String.init) ?? path, systemImage: "folder")
        case .file(let group):
            Button {
                store.openSearchFile(group.id)
            } label: {
                HStack(spacing: 8) {
                    if let icon = WorkspaceIcons.image(for: group.id) {
                        Image(nsImage: icon).resizable().interpolation(.high).frame(width: 20, height: 20)
                    }
                    Text(group.id.split(separator: "/").last.map(String.init) ?? group.id)
                    Spacer(minLength: 0)
                    if store.documents.first(where: { $0.id == group.id })?.externalChange == true {
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
                    OutlineGroup(sidebarTree, children: \.children) { node in
                        sidebarRow(node)
                    }
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
