import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @ObservedObject var store: WorkspaceStore
    @State private var choosingWorkspace = false

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
                    ForEach(store.searchResults, id: \.id) { document in
                        Button {
                            store.openDocument(document.id)
                        } label: {
                            HStack(spacing: 8) {
                                if let icon = WorkspaceIcons.image(for: document.id) {
                                    Image(nsImage: icon)
                                        .resizable()
                                        .interpolation(.high)
                                        .frame(width: 20, height: 20)
                                }
                                Text(document.title)
                                Spacer(minLength: 0)
                                if document.dirty {
                                    Circle().fill(.secondary).frame(width: 6, height: 6)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(store.activeDocument?.id == document.id ? Color.accentColor.opacity(0.12) : Color.clear)
                    }
                }
                .overlay {
                    if store.workspaceName != nil && store.searchResults.isEmpty {
                        Text(store.searchQuery.isEmpty ? "No files" : "No matching files")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle(store.workspaceName ?? "Workspace")
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
                    NoWrapTextEditor(documentID: active.id, text: Binding(
                        get: { store.editorText },
                        set: { store.changeDocument($0) }
                    ))
                    .navigationTitle(active.title)
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
        .frame(minWidth: 680, minHeight: 420)
    }
}
