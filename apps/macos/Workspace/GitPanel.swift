import SwiftUI

private let gitColors: [Color] = [.blue, .orange, .purple, .green, .pink, .teal]

struct GitPanel: View {
    @ObservedObject var store: WorkspaceStore
    @State private var sideBySide = true
    @State private var mergeDraft = ""
    @State private var message = ""
    @State private var amend = false
    @State private var signoff = false
    @State private var reviewing = false
    @State private var selectedLines: Set<Int> = []
    @State private var form: GitPanelState.Action?
    @State private var first = ""
    @State private var second = ""
    @State private var option = false
    @State private var confirmation: (() -> Void)?
    @State private var confirming = false
    @State private var filter = ""
    @State private var requestedRoot: String?
    private var busy: Bool { store.backendBusy }

    var body: some View {
        GeometryReader { geometry in
            VSplitView {
                changes.frame(minHeight: 160, idealHeight: geometry.size.height / 2)
                history.frame(minHeight: 160, idealHeight: geometry.size.height / 2)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Source Control")
        .accessibilityIdentifier("gitPanel")
        .onAppear {
            store.setGitVisible(true)
            loadRepositoryIfNeeded()
        }
        .onDisappear { store.setGitVisible(false) }
        .onChange(of: store.workspaceRoot) { _, _ in loadRepositoryIfNeeded() }
        .onChange(of: store.revision) { _, _ in loadRepositoryIfNeeded() }
        .onChange(of: store.git.loaded) { _, loaded in
            if !loaded { requestedRoot = nil; loadRepositoryIfNeeded() }
        }
        .sheet(item: $form) { action in
            VStack(alignment: .leading, spacing: 12) {
                Text(action.title).font(.headline)
                if !action.first.isEmpty { TextField(action.first, text: $first).textFieldStyle(.roundedBorder) }
                if !action.second.isEmpty { TextField(action.second, text: $second).textFieldStyle(.roundedBorder) }
                if !action.option.isEmpty { Toggle(action.option, isOn: $option) }
                if action.destructive { Text(workspaceGitConfirmation().body).font(.caption).foregroundStyle(.secondary) }
                HStack {
                    Spacer()
                    Button("Cancel") { form = nil }.keyboardShortcut(.cancelAction)
                    Button(action.title) { store.gitAction(action.id, first, second, option); form = nil }
                        .keyboardShortcut(.defaultAction)
                }
            }.padding().frame(width: 440)
        }
        .alert(workspaceGitConfirmation().title, isPresented: $confirming) {
            Button(workspaceGitConfirmation().confirm, role: .destructive) { confirmation?(); confirmation = nil }
            Button(workspaceGitConfirmation().cancel, role: .cancel) { confirmation = nil }
        } message: { Text(workspaceGitConfirmation().body) }
        .sheet(isPresented: $reviewing) { reviewPanel }
    }

    private var changes: some View {
        VStack(alignment: .leading, spacing: 8) {
            if store.git.roots.count > 1 {
                Picker("Repository", selection: Binding(get:{store.git.selected_root ?? ""},set:{store.gitAction("select_repository",$0)})) {ForEach(store.git.roots,id:\.self){Text($0).tag($0)}}
            }
            HStack(spacing: 6) {
                Menu {
                    ForEach(store.git.repository?.branches ?? [], id: \.name) { branch in
                        Button((branch.current ? "✓ " : "") + branch.name) { store.gitAction("checkout", branch.name) }
                    }
                } label: {
                    Label(store.git.repository?.branch ?? "Source Control", systemImage: "arrow.triangle.branch")
                        .lineLimit(1).truncationMode(.middle)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(minWidth: 0, maxWidth: .infinity)
                .accessibilityIdentifier("gitBranch")
                if busy { ProgressView().controlSize(.small) }
                Button("Refresh", systemImage: "arrow.clockwise") { store.refreshGit() }.labelStyle(.iconOnly)
                    .fixedSize().accessibilityIdentifier("gitRefresh")
                Menu {
                    Menu("Stashes") {
                        ForEach(store.git.repository?.stashes ?? [], id: \.index) { stash in
                            Menu("[\(stash.index)] \(stash.subject)") {
                                Button("Apply") { store.gitAction("stash_apply",String(stash.index)) }
                                Button("Pop") { store.gitAction("stash_apply",String(stash.index),"",true) }
                                Button("Delete…",role:.destructive) { confirm {store.gitAction("stash_drop",String(stash.index))} }
                            }
                        }
                    }
                    Menu("Remotes") {
                        ForEach(store.git.repository?.remotes ?? [], id: \.name) { remote in
                            Menu(remote.name) {
                                Text(remote.url)
                                Button("Fetch") {store.gitAction("fetch",remote.name)}
                                Button("Publish Branch") {store.gitAction("publish",remote.name)}
                            }
                        }
                    }
                    Menu("Worktrees") {
                        ForEach(store.git.repository?.worktrees ?? [], id: \.path) {worktree in
                            Button("\(worktree.branch) — \(worktree.path)") {store.openWorkspace(URL(fileURLWithPath:worktree.path))}
                        }
                    }
                    Divider()
                    ForEach(store.git.actions) { action in Button(action.title) { open(action) } }
                } label: { Image(systemName: "ellipsis").frame(width: 18) }
                .menuIndicator(.hidden).fixedSize()
                .accessibilityLabel("Git Actions").accessibilityIdentifier("gitActions")
            }.disabled(busy)
            if let repo = store.git.repository {
                HStack {
                    Text("↑ \(repo.ahead)  ↓ \(repo.behind)").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Sync", systemImage: "arrow.triangle.2.circlepath") { store.gitAction("sync") }.labelStyle(.iconOnly)
                }.disabled(busy)
                TextField("Commit message", text: $message, axis: .vertical).lineLimit(2...4).textFieldStyle(.roundedBorder)
                HStack {
                    Toggle("Amend", isOn: $amend).toggleStyle(.checkbox)
                    Toggle("Sign off", isOn: $signoff).toggleStyle(.checkbox)
                    Spacer()
                    Button("Commit") { store.gitAction("commit", message, signoff ? "signoff" : "", amend) }
                        .disabled(message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
                }.font(.caption)
                if let operation = repo.operation {
                    HStack {
                        Text("\(operation) in progress").font(.caption)
                        Button("Continue") { store.gitAction("continue") }
                        Button("Abort") { confirm { store.gitAction("abort") } }
                    }.disabled(busy)
                }
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        group("Staged Changes", changes: repo.changes.filter(\.staged), staged: true)
                        group("Changes", changes: repo.changes.filter(\.unstaged), staged: false)
                        if repo.changes.isEmpty { Text("Working tree clean").foregroundStyle(.secondary) }
                    }
                }
            } else {
                Text("Initialize or clone a repository from Git Actions.").foregroundStyle(.secondary)
                Spacer()
            }
            if let error = store.errorMessage { Text(error).font(.caption).foregroundStyle(.red).lineLimit(4).textSelection(.enabled) }
        }.padding(10)
    }

    private func loadRepositoryIfNeeded() {
        guard let root = store.workspaceRoot, requestedRoot != root, !busy else { return }
        requestedRoot = root
        store.refreshGit()
    }

    private func group(_ title: String, changes: [GitPanelState.Change], staged: Bool) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack {
                Text("\(title) (\(changes.count))").font(.caption.bold())
                Spacer()
                Button(staged ? "Unstage All" : "Stage All", systemImage: staged ? "minus" : "plus") { store.gitAction(staged ? "unstage" : "stage") }
                    .labelStyle(.iconOnly).disabled(changes.isEmpty || busy)
            }
            ForEach(changes, id: \.path) { change in
                HStack(spacing: 5) {
                    if let image = WorkspaceIcons.image(for: change.path) { Image(nsImage: image).resizable().frame(width: 14, height: 14) }
                    Button { store.showGitDiff(change.path, staged: staged); selectedLines = []; reviewing = true } label: {
                        Text(change.path).lineLimit(1).truncationMode(.middle).frame(maxWidth: .infinity, alignment: .leading)
                    }.buttonStyle(.plain)
                    Text(change.conflicted ? "!" : (staged ? change.index : change.worktree)).foregroundStyle(change.conflicted ? .red : .secondary)
                    Button(staged ? "Unstage" : "Stage", systemImage: staged ? "minus" : "plus") { store.gitAction(staged ? "unstage" : "stage", change.path) }.labelStyle(.iconOnly)
                }.font(.caption).padding(.vertical, 3).disabled(busy)
                .contextMenu {
                    if change.conflicted { Button("Resolve in Merge Editor") { inspect("resolve", change.path) } }
                    Button(staged ? "Unstage" : "Stage") { store.gitAction(staged ? "unstage" : "stage", change.path) }
                    Button("File History") { inspect("file_history", change.path) }
                    Button("Blame") { inspect("blame", change.path) }
                    Button("Ignore") { store.gitAction("ignore", change.path) }
                    if !staged { Button("Discard Changes…", role: .destructive) { confirm { store.gitAction("discard", change.path) } } }
                }
            }
        }
    }

    private var history: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack { Label("Source Control Graph", systemImage: "point.3.connected.trianglepath.dotted").font(.caption.bold()); Spacer() }
            TextField("Highlight in history", text: $filter).textFieldStyle(.roundedBorder)
            ScrollView([.horizontal, .vertical]) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(store.git.history, id: \.commit.id) { row in
                        HStack(spacing: 6) {
                            Canvas { context, size in
                                for edge in row.edges {
                                    var path = Path(); let a = CGPoint(x: CGFloat(edge.from) * 14 + 7, y: CGFloat(edge.start) * size.height / 2); let b = CGPoint(x: CGFloat(edge.to) * 14 + 7, y: CGFloat(edge.end) * size.height / 2)
                                    path.move(to: a); path.addCurve(to: b, control1: CGPoint(x: a.x, y: (a.y + b.y) / 2), control2: CGPoint(x: b.x, y: (a.y + b.y) / 2)); context.stroke(path, with: .color(gitColors[edge.color % gitColors.count]), lineWidth: 2)
                                }
                                let dot = CGRect(x: CGFloat(row.lane) * 14 + 3, y: size.height / 2 - 4, width: 8, height: 8)
                                context.fill(Path(ellipseIn: dot), with: .color(gitColors[row.lane % gitColors.count]))
                            }.frame(width: CGFloat(max(2, row.columns)) * 14, height: 52)
                            VStack(alignment: .leading, spacing: 2) {
                                if !row.commit.refs.isEmpty { Text(row.commit.refs).font(.caption2.bold()).foregroundStyle(.tint).lineLimit(1) }
                                Text(row.commit.subject).font(.caption).lineLimit(1)
                                Text("\(row.commit.id.prefix(7)) · \(row.commit.author) · \(row.commit.date.prefix(10))").font(.caption2).foregroundStyle(.secondary)
                            }
                        }.frame(height: 52).opacity(filter.isEmpty || (row.commit.subject + row.commit.author + row.commit.refs).localizedCaseInsensitiveContains(filter) ? 1 : 0.3)
                        .contentShape(Rectangle()).onTapGesture { inspect("show_commit", row.commit.id) }
                        .contextMenu {
                            Button("Show Commit") { inspect("show_commit", row.commit.id) }
                            Button("Checkout Commit") { store.gitAction("checkout", row.commit.id) }
                            Button("Cherry-pick") { store.gitAction("cherry_pick", row.commit.id) }
                            Button("Revert Commit") { store.gitAction("revert", row.commit.id) }
                            if let action = store.git.actions.first(where: { $0.id == "branch" }) { Button("Create Branch Here…") { open(action); second = row.commit.id } }
                        }
                    }
                    if store.git.history.count >= store.git.history_limit {
                        Button("Load More Commits") { store.gitLoadMore() }.disabled(busy)
                    }
                }
            }
        }.padding(10)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Source Control Graph")
        .accessibilityIdentifier("gitGraph")
    }

    private var reviewPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(store.git.inspection?.title ?? store.git.diff?.path ?? "Git Changes").font(.headline).lineLimit(1)
                Spacer()
                if store.git.diff != nil {
                    Toggle("Side by Side", isOn: $sideBySide).toggleStyle(.checkbox)
                    Button(store.git.diff?.staged == true ? "Unstage Selected Lines" : "Stage Selected Lines") { store.gitStageLines(selectedLines); reviewing = false }.disabled(selectedLines.isEmpty || busy)
                }
                Button("Done") { reviewing = false }.keyboardShortcut(.cancelAction)
            }
            if busy { ProgressView("Loading…") }
            else if let error = store.errorMessage { Text(error).textSelection(.enabled) }
            else if let conflict = store.git.inspection?.conflict {
                HSplitView {
                    VStack { Text("Current"); ScrollView { Text(conflict.current).font(.system(.caption, design: .monospaced)).textSelection(.enabled) } }
                    VStack { Text("Incoming"); ScrollView { Text(conflict.incoming).font(.system(.caption, design: .monospaced)).textSelection(.enabled) } }
                }.frame(height: 170)
                HStack {
                    Button("Accept Current") { mergeDraft = conflict.accept_current }
                    Button("Accept Incoming") { mergeDraft = conflict.accept_incoming }
                    Button("Accept Both") { mergeDraft = conflict.accept_both }
                    Spacer()
                    Button("Save and Stage Result") { store.gitResolve(mergeDraft); reviewing = false }
                }
                TextEditor(text: $mergeDraft).font(.system(.body, design: .monospaced))
                    .onAppear { mergeDraft = conflict.expected }
            }
            else if sideBySide, let before=store.git.diff?.before, let after=store.git.diff?.after {
                HSplitView {
                    VStack(alignment:.leading) { Text("Before").font(.caption.bold());ScrollView([.horizontal,.vertical]) {Text(before).font(.system(.caption,design:.monospaced)).textSelection(.enabled).frame(maxWidth:.infinity,alignment:.topLeading)} }
                    VStack(alignment:.leading) { Text("After").font(.caption.bold());ScrollView([.horizontal,.vertical]) {Text(after).font(.system(.caption,design:.monospaced)).textSelection(.enabled).frame(maxWidth:.infinity,alignment:.topLeading)} }
                }
            }
            else if !store.git.patch_lines.isEmpty && store.git.diff != nil {
                ScrollView([.horizontal, .vertical]) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(store.git.patch_lines, id: \.index) { line in
                            HStack(alignment: .top, spacing: 4) {
                                if line.changed { Toggle("", isOn: Binding(get: { selectedLines.contains(line.index) }, set: { if $0 { selectedLines.insert(line.index) } else { selectedLines.remove(line.index) } })).labelsHidden().toggleStyle(.checkbox) }
                                else { Color.clear.frame(width: 16, height: 1) }
                                Text(line.text.trimmingCharacters(in: .newlines)).font(.system(.caption, design: .monospaced)).foregroundStyle(line.text.hasPrefix("+") ? .green : (line.text.hasPrefix("-") ? .red : .primary)).textSelection(.enabled)
                            }
                        }
                    }
                }
            } else {
                ScrollView([.horizontal, .vertical]) { Text(store.git.inspection?.text ?? store.git.diff?.text ?? "").font(.system(.body, design: .monospaced)).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .topLeading) }
            }
        }.padding().frame(minWidth: 800, minHeight: 550)
    }
    private func open(_ action: GitPanelState.Action) { first = ""; second = ""; option = false; form = action }
    private func confirm(_ action: @escaping () -> Void) { confirmation = action; confirming = true }
    private func inspect(_ action: String, _ value: String) { store.gitAction(action, value); selectedLines = []; reviewing = true }
}
