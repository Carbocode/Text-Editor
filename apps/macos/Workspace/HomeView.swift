import SwiftUI
import UniformTypeIdentifiers

struct HomeView: View {
    @ObservedObject var store: WorkspaceStore
    @Environment(\.openWindow) private var openWindow
    @State private var choosingWorkspace = false

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(spacing: 16) {
                Image(systemName: "folder.fill").font(.system(size: 42)).foregroundStyle(.tint)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Native Workspace").font(.largeTitle.bold())
                    Text("A place for your files and ideas.").foregroundStyle(.secondary)
                }
            }
            HStack {
                Button("Open Folder…", systemImage: "folder.badge.plus") { choosingWorkspace = true }
                    .buttonStyle(.borderedProminent).controlSize(.large)
                if store.workspaceName != nil {
                    Button("Resume Workspace", systemImage: "arrow.right") { openWindow(id: "workspace") }
                        .controlSize(.large)
                }
            }
            Divider()
            Text("Recent Workspaces").font(.headline)
            if store.recentWorkspaces.isEmpty {
                ContentUnavailableView("Your next workspace starts here", systemImage: "folder",
                                       description: Text("Open a folder. It will appear here for quick access."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(store.recentWorkspaces, id: \.root) { workspace in
                            Button { open(URL(fileURLWithPath: workspace.root, isDirectory: true)) } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: "folder").font(.title2).foregroundStyle(.tint)
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(workspace.name).font(.headline)
                                        Text(workspace.root).font(.caption).foregroundStyle(.secondary)
                                            .lineLimit(1).truncationMode(.middle)
                                    }
                                    Spacer()
                                    Image(systemName: "chevron.right").foregroundStyle(.secondary)
                                }
                                .padding(14).frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.bordered).help(workspace.root)
                        }
                    }
                }
            }
            Spacer(minLength: 0)
            if let error = store.errorMessage { Text(error).foregroundStyle(.red).textSelection(.enabled) }
        }
        .padding(.top, 24)
        .padding(28)
        .frame(minWidth: 520, minHeight: 440)
        .toolbar {
            ToolbarSpacer(.flexible, placement: .primaryAction)
            ToolbarItem(placement: .primaryAction) {
                ApplicationMenu(store: store, openFolder: { choosingWorkspace = true })
            }
        }
        .fileImporter(isPresented: $choosingWorkspace, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { open(url) }
        }
    }

    private func open(_ url: URL) {
        store.openWorkspace(url)
        if store.workspaceRoot == url.path { openWindow(id: "workspace") }
    }
}
