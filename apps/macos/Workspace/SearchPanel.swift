import SwiftUI

/// Native presentation only; matching, filtering and replacement live in Rust.
struct SearchPanel: View {
    @ObservedObject var store: WorkspaceStore
    @State private var showDetails = false
    private enum Field: Hashable { case query, replacement }
    @FocusState private var focused: Field?
    private var confirmation: Confirmation {
        workspaceReplacementConfirmation(occurrences: store.replacementOccurrences, files: store.replacementFiles)
    }

    private func setting<T>(_ key: WritableKeyPath<SearchOptions, T>) -> Binding<T> {
        Binding(get: { store.searchOptions[keyPath: key] }, set: { value in
            var options = store.searchOptions
            options[keyPath: key] = value
            store.configureSearch(options)
        })
    }

    private func option(_ label: String, help: String, key: WritableKeyPath<SearchOptions, Bool>) -> some View {
        Toggle(label, isOn: setting(key))
            .toggleStyle(.button).controlSize(.small).help(help)
            .accessibilityLabel(help)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 4) {
                Button { store.showReplace.toggle() } label: {
                    Image(systemName: store.showReplace ? "chevron.down" : "chevron.right")
                }.help("Toggle Replace").accessibilityLabel("Toggle Replace")
                TextField("Search in files", text: Binding(get: { store.searchFieldText }, set: store.setSearchQuery))
                    .textFieldStyle(.roundedBorder).accessibilityIdentifier("searchQuery")
                    .focused($focused, equals: .query)
                    .onSubmit { store.refreshSearch() }
                option("Aa", help: "Match Case", key: \.matchCase)
                option("ab", help: "Match Whole Word", key: \.wholeWord)
                option(".*", help: "Use Regular Expression", key: \.regex)
            }
            if store.showReplace {
                HStack(spacing: 4) {
                    TextField("Replace", text: $store.replacementText).textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("searchReplacement")
                        .focused($focused, equals: .replacement)
                    Toggle("AB", isOn: $store.preserveReplacementCase).toggleStyle(.button)
                        .help("Preserve Case").accessibilityLabel("Preserve Case")
                    Button { store.prepareReplacement() } label: { Image(systemName: "arrow.triangle.2.circlepath") }
                        .help("Replace All…").accessibilityLabel("Replace All")
                        .disabled(store.searchResults.isEmpty || store.searchInProgress || store.searchTruncated || store.backendBusy)
                }
            }
            HStack(spacing: 8) {
                Button { store.refreshSearch() } label: { Image(systemName: "arrow.clockwise") }.help("Refresh Search")
                if store.searchInProgress {
                    ProgressView().controlSize(.small)
                    Button { store.cancelSearch() } label: { Image(systemName: "stop.fill") }.help("Cancel Search")
                }
                Button { store.setSearchQuery("") } label: { Image(systemName: "clear") }.help("Clear Search")
                Spacer(minLength: 0)
                Button { showDetails.toggle() } label: { Image(systemName: "ellipsis") }
                    .help("Toggle Search Details").accessibilityLabel("Toggle Search Details")
            }
            if showDetails {
                Text("files to include").font(.caption).foregroundStyle(.secondary)
                TextField("e.g. *.rs, src/**", text: setting(\.include)).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Files to Include")
                Toggle("Search only in Open Editors", isOn: setting(\.onlyOpenEditors)).font(.caption)
                Text("files to exclude").font(.caption).foregroundStyle(.secondary)
                TextField("e.g. target, **/*.lock", text: setting(\.exclude)).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Files to Exclude")
                Toggle("Use Ignore Files", isOn: setting(\.useIgnoreFiles)).font(.caption)
            }
            if let error = store.searchError {
                Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled)
            } else if !store.searchQuery.isEmpty {
                Text("\(store.searchResults.count) results in \(Set(store.searchResults.map(\.id)).count) files")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if store.searchTruncated {
                Text("Results are incomplete. Refresh or refine the search before replacing.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.borderless).controlSize(.small)
        .padding(.vertical, 8)
        .onAppear { focused = store.showReplace ? .replacement : .query }
        .onChange(of: store.searchFocusRevision) { _, _ in focused = store.showReplace ? .replacement : .query }
        .sheet(isPresented: Binding(get: { store.replacementPreview != nil }, set: { if !$0 { store.discardReplacement() } })) {
            VStack(alignment: .leading, spacing: 12) {
                Text(confirmation.title).font(.headline)
                Text(confirmation.body).font(.caption)
                ScrollView([.vertical, .horizontal]) {
                    Text(store.replacementPreview ?? "").font(.system(.body, design: .monospaced))
                        .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                if let error = store.errorMessage { Text(error).foregroundStyle(.red).font(.caption) }
                HStack {
                    Spacer()
                    Button(confirmation.cancel, action: store.discardReplacement).keyboardShortcut(.cancelAction)
                    Button(confirmation.confirm, action: store.applyReplacement).keyboardShortcut(.defaultAction)
                        .disabled(store.backendBusy)
                }
            }.padding(20).frame(minWidth: 560, idealWidth: 680, minHeight: 360, idealHeight: 480)
        }
    }
}
