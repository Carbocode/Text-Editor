import AppKit
import SwiftUI

private typealias GitChange = GitPanelState.Change

private let gitColors: [Color] = [
    Color(red: 0.24, green: 0.56, blue: 0.98), Color(red: 0.96, green: 0.55, blue: 0.20),
    Color(red: 0.62, green: 0.42, blue: 0.94), Color(red: 0.25, green: 0.72, blue: 0.47),
    Color(red: 0.93, green: 0.36, blue: 0.56), Color(red: 0.18, green: 0.70, blue: 0.76),
    Color(red: 0.85, green: 0.70, blue: 0.16), Color(red: 0.55, green: 0.60, blue: 0.68),
]
private func laneColor(_ index: Int) -> Color { gitColors[index % gitColors.count] }

private enum GitMetrics {
    static let row: CGFloat = 24
    static let lane: CGFloat = 10
    /// Wider histories keep their geometry but only the first lanes are drawn,
    /// so the gutter never pushes row text out of a narrow sidebar.
    static let maxLanes = 8
    static let badge: CGFloat = 110
    static func gutter(_ lanes: Int) -> CGFloat { CGFloat(max(1, min(lanes, maxLanes))) * lane + 4 }
}

/// Status letters and colors are presentation; Git classification comes from Rust.
private func changeStatus(_ change: GitChange, staged: Bool) -> (String, Color) {
    if change.conflicted { return ("!", .red) }
    return fileStatus(staged ? change.index : change.worktree)
}
private func fileStatus(_ code: String) -> (String, Color) {
    switch code {
    case "?": ("U", .green)
    case "A": ("A", .green)
    case "D": ("D", .red)
    case "R", "C": (code, .blue)
    default: (code, .orange)
    }
}

private let isoDate: ISO8601DateFormatter = ISO8601DateFormatter()
private func gitDate(_ value: String) -> Date? { isoDate.date(from: value) }
/// Compact age for dense rows ("now", "5m", "3h", "2d", "4w"), then the date.
private func shortDate(_ value: String) -> String {
    guard let date = gitDate(value) else { return String(value.prefix(10)) }
    let seconds = max(0, Date().timeIntervalSince(date))
    switch seconds {
    case ..<60: return "now"
    case ..<3600: return "\(Int(seconds / 60))m"
    case ..<86_400: return "\(Int(seconds / 3600))h"
    case ..<604_800: return "\(Int(seconds / 86_400))d"
    case ..<2_419_200: return "\(Int(seconds / 604_800))w"
    default: return date.formatted(.dateTime.day().month(.abbreviated).year(.twoDigits))
    }
}
private func copy(_ text: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(text, forType: .string)
}

struct GitPanel: View {
    @ObservedObject var store: WorkspaceStore
    @State private var message = ""
    @State private var amend = false
    @State private var signoff = false
    @State private var reviewing = false
    @State private var form: GitPanelState.Action?
    @State private var first = ""
    @State private var second = ""
    @State private var option = false
    @State private var confirmation: (() -> Void)?
    @State private var confirming = false
    @State private var filter = ""
    @State private var requestedRoot: String?
    @State private var collapsed: Set<String> = []
    @State private var selectedCommit: String?
    @State private var committing = false
    private var busy: Bool { store.backendBusy }
    private var repo: GitPanelState.Repository? { store.git.repository }

    var body: some View {
        GeometryReader { geometry in
            VSplitView {
                changes.frame(minWidth: 0, maxWidth: geometry.size.width, minHeight: 160, idealHeight: geometry.size.height / 2)
                history.frame(minWidth: 0, maxWidth: geometry.size.width, minHeight: 160, idealHeight: geometry.size.height / 2)
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
        .onChange(of: repo?.head) { _, _ in
            if committing { committing = false; message = ""; amend = false }
        }
        .onChange(of: store.errorMessage) { _, error in if error != nil { committing = false } }
        .onChange(of: store.git.loaded) { _, loaded in
            if !loaded { requestedRoot = nil; selectedCommit = nil; loadRepositoryIfNeeded() }
        }
        .sheet(item: $form) { action in actionForm(action) }
        .alert(workspaceGitConfirmation().title, isPresented: $confirming) {
            Button(workspaceGitConfirmation().confirm, role: .destructive) { confirmation?(); confirmation = nil }
            Button(workspaceGitConfirmation().cancel, role: .cancel) { confirmation = nil }
        } message: { Text(workspaceGitConfirmation().body) }
        .sheet(isPresented: $reviewing) { GitReviewSheet(store: store, done: { reviewing = false }) }
    }

    // MARK: Changes pane

    private var changes: some View {
        VStack(alignment: .leading, spacing: 6) {
            if store.git.roots.count > 1 {
                Picker("Repository", selection: Binding(get: { store.git.selected_root ?? "" }, set: { store.gitAction("select_repository", $0) })) {
                    ForEach(store.git.roots, id: \.self) { Text(workspacePathName(value: $0)).tag($0).help($0) }
                }
                .labelsHidden().controlSize(.small).frame(minWidth: 0, maxWidth: .infinity)
            }
            toolbar
            if let repo {
                if let operation = repo.operation { operationBanner(operation) }
                commitBox(repo)
                changeList(repo)
            } else {
                emptyRepository
            }
            if let error = store.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption).foregroundStyle(.red).lineLimit(6).truncationMode(.middle).textSelection(.enabled)
                    .help(error)
                    .padding(6).frame(maxWidth: .infinity, alignment: .leading)
                    .background(.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
    }

    private var toolbar: some View {
        HStack(spacing: 4) {
            branchMenu
            if busy { ProgressView().controlSize(.mini) }
            if let repo, repo.ahead + repo.behind > 0 {
                Button { store.gitAction("sync") } label: {
                    HStack(spacing: 2) {
                        if repo.behind > 0 { Text("↓\(repo.behind)") }
                        if repo.ahead > 0 { Text("↑\(repo.ahead)") }
                    }.font(.caption.monospacedDigit())
                }
                .buttonStyle(.borderless).fixedSize()
                .help("Sync: pull \(repo.behind), push \(repo.ahead)")
            }
            Button("Refresh", systemImage: "arrow.clockwise") { store.refreshGit() }
                .labelStyle(.iconOnly).buttonStyle(.borderless).fixedSize()
                .help("Refresh").accessibilityIdentifier("gitRefresh")
            actionsMenu
        }
        .disabled(busy)
    }

    private var branchMenu: some View {
        let branches = repo?.branches ?? []
        return Menu {
            Section("Branches") {
                ForEach(branches.filter { !$0.remote }, id: \.name) { branch in
                    Button { store.gitAction("checkout", branch.name) } label: {
                        Label(branch.name, systemImage: branch.current ? "checkmark" : "arrow.triangle.branch")
                    }.disabled(branch.current)
                }
            }
            let remotes = branches.filter(\.remote)
            if !remotes.isEmpty {
                Menu("Remote Branches") {
                    ForEach(remotes, id: \.name) { branch in Button(branch.name) { store.gitAction("checkout", branch.name) } }
                }
            }
            Divider()
            Button("New Branch…", systemImage: "plus") { open("branch") }
            Button("Checkout Branch or Commit…") { open("checkout") }
        } label: {
            Label(repo?.branch ?? "Source Control", systemImage: "arrow.triangle.branch")
                .lineLimit(1).truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .menuStyle(.borderlessButton)
        .frame(minWidth: 0, maxWidth: .infinity)
        .help(repo.map { "\($0.branch) — \($0.root)" } ?? "Source Control")
        .accessibilityIdentifier("gitBranch")
    }

    private var actionsMenu: some View {
        Menu {
            Section("Remote") {
                Button("Fetch", systemImage: "arrow.down.circle") { store.gitAction("fetch") }
                Button("Pull", systemImage: "arrow.down") { store.gitAction("pull") }
                Button("Pull (Rebase)") { store.gitAction("pull", "", "", true) }
                Button("Push", systemImage: "arrow.up") { store.gitAction("push") }
                Button("Sync", systemImage: "arrow.triangle.2.circlepath") { store.gitAction("sync") }
            }
            Menu("Stashes") {
                Button("Stash All Changes…") { open("stash") }
                if !(repo?.stashes.isEmpty ?? true) { Divider() }
                ForEach(repo?.stashes ?? [], id: \.index) { stash in
                    Menu("\(stash.index): \(stash.subject)") {
                        Button("Apply") { store.gitAction("stash_apply", String(stash.index)) }
                        Button("Pop") { store.gitAction("stash_apply", String(stash.index), "", true) }
                        Button("Delete…", role: .destructive) { confirm { store.gitAction("stash_drop", String(stash.index)) } }
                    }
                }
            }
            Menu("Remotes") {
                ForEach(repo?.remotes ?? [], id: \.name) { remote in
                    Menu(remote.name) {
                        Button("Copy URL") { copy(remote.url) }
                        Button("Fetch") { store.gitAction("fetch", remote.name) }
                        Button("Publish Current Branch") { store.gitAction("publish", remote.name) }
                        Divider()
                        Button("Remove…", role: .destructive) { confirm { store.gitAction("remote_remove", remote.name) } }
                    }
                }
                if !(repo?.remotes.isEmpty ?? true) { Divider() }
                Button("Add Remote…") { open("remote_add") }
            }
            Menu("Worktrees") {
                ForEach(repo?.worktrees ?? [], id: \.path) { worktree in
                    Button("\(worktree.branch) — \(worktree.path)") { store.openWorkspace(URL(fileURLWithPath: worktree.path)) }
                }
                if !(repo?.worktrees.isEmpty ?? true) { Divider() }
                Button("Create Worktree…") { open("worktree_add") }
                Button("Remove Worktree…") { open("worktree_remove") }
            }
            Divider()
            ForEach(groups, id: \.self) { group in
                Menu(group) {
                    ForEach(store.git.actions.filter { $0.group == group }) { action in
                        Button(action.title + (action.first.isEmpty && action.second.isEmpty && action.option.isEmpty ? "" : "…"),
                               role: action.destructive ? .destructive : nil) { open(action) }
                    }
                }
            }
        } label: { Image(systemName: "ellipsis.circle").frame(width: 18) }
        .menuIndicator(.hidden).menuStyle(.borderlessButton).fixedSize()
        .help("Git Actions")
        .accessibilityLabel("Git Actions").accessibilityIdentifier("gitActions")
    }

    private var groups: [String] {
        store.git.actions.reduce(into: [String]()) { if !$0.contains($1.group) { $0.append($1.group) } }
    }

    private func operationBanner(_ operation: String) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "exclamationmark.arrow.triangle.2.circlepath").foregroundStyle(.orange)
            Text("\(operation.capitalized) in progress").font(.caption.bold()).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: 0)
            Button("Continue") { store.gitAction("continue") }
            Button("Abort") { confirm { store.gitAction("abort") } }
        }
        .controlSize(.small).disabled(busy)
        .padding(6).background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 6))
    }

    private func commitBox(_ repo: GitPanelState.Repository) -> some View {
        let staged = repo.changes.filter(\.staged).count
        let ready = !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (staged > 0 || amend) && !busy
        let title = amend ? "Amend Commit" : staged > 0 ? "Commit \(staged) File\(staged == 1 ? "" : "s")" : "Commit"
        return VStack(spacing: 5) {
            TextField("Message (⌘↩ to commit)", text: $message, axis: .vertical)
                .lineLimit(1...6).textFieldStyle(.plain).font(.callout)
                .padding(.horizontal, 7).padding(.vertical, 5)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(.separator))
                .accessibilityIdentifier("gitCommitMessage")
            HStack(spacing: 4) {
                Button { commit() } label: {
                    Label(title, systemImage: "checkmark").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent).controlSize(.regular)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!ready)
                .help(staged == 0 && !amend ? "Stage changes to commit" : title)
                Menu {
                    Toggle("Amend Last Commit", isOn: $amend)
                    Toggle("Add Signed-off-by", isOn: $signoff)
                    Divider()
                    Button("Undo Last Commit…") { confirm { store.gitAction("undo_commit") } }
                    Button("Stash All Changes…") { open("stash") }
                } label: { Image(systemName: "chevron.down") }
                .menuIndicator(.hidden).menuStyle(.borderlessButton).fixedSize()
                .help("Commit Options")
            }
            if amend || signoff {
                HStack(spacing: 6) {
                    if amend { tag("Amend", "arrow.uturn.backward") { amend = false } }
                    if signoff { tag("Sign-off", "signature") { signoff = false } }
                    Spacer()
                }
            }
        }
    }

    private func tag(_ title: String, _ icon: String, remove: @escaping () -> Void) -> some View {
        Button(action: remove) {
            HStack(spacing: 3) { Image(systemName: icon); Text(title); Image(systemName: "xmark").imageScale(.small) }
                .font(.caption2).padding(.horizontal, 6).padding(.vertical, 2)
                .background(.tint.opacity(0.15), in: Capsule())
        }.buttonStyle(.plain)
    }

    private func commit() {
        // The draft stays until HEAD moves, so a rejected commit keeps the message.
        committing = true
        store.gitAction("commit", message, signoff ? "signoff" : "", amend)
    }

    private func changeList(_ repo: GitPanelState.Repository) -> some View {
        let conflicts = repo.changes.filter(\.conflicted)
        let staged = repo.changes.filter { $0.staged && !$0.conflicted }
        let unstaged = repo.changes.filter { $0.unstaged && !$0.conflicted }
        return ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if !conflicts.isEmpty { section("Merge Changes", conflicts, staged: false) }
                if !staged.isEmpty { section("Staged Changes", staged, staged: true) }
                section("Changes", unstaged, staged: false)
                if repo.changes.isEmpty {
                    Label("No changes", systemImage: "checkmark.circle")
                        .font(.caption).foregroundStyle(.secondary).padding(.vertical, 6).padding(.leading, 18)
                }
            }
        }
        .scrollIndicators(.automatic)
    }

    private func section(_ title: String, _ changes: [GitChange], staged: Bool) -> some View {
        let open = !collapsed.contains(title)
        return Group {
            GitSectionHeader(title: title, count: changes.count, open: open, staged: staged, busy: busy,
                toggle: { if open { collapsed.insert(title) } else { collapsed.remove(title) } },
                all: { store.gitAction(staged ? "unstage" : "stage") })
            if open {
                ForEach(changes, id: \.path) { change in
                    GitChangeRow(change: change, staged: staged, busy: busy,
                        review: { review(change, staged: staged) },
                        openFile: { openFile(change.path) },
                        stage: { store.gitAction(staged ? "unstage" : "stage", change.path) },
                        discard: { confirm { store.gitAction("discard", change.path) } })
                    .contextMenu { changeMenu(change, staged: staged) }
                }
            }
        }
    }

    @ViewBuilder private func changeMenu(_ change: GitChange, staged: Bool) -> some View {
        if change.conflicted { Button("Resolve in Merge Editor…") { inspect("resolve", change.path) } }
        Button("Open Changes") { review(change, staged: staged) }
        Button("Open File") { openFile(change.path) }
        Divider()
        Button(staged ? "Unstage" : "Stage") { store.gitAction(staged ? "unstage" : "stage", change.path) }
        if !staged { Button("Discard Changes…", role: .destructive) { confirm { store.gitAction("discard", change.path) } } }
        Divider()
        Button("File History") { inspect("file_history", change.path) }
        Button("Blame") { inspect("blame", change.path) }
        Button("Add to .gitignore") { store.gitAction("ignore", change.path) }
        Divider()
        Button("Copy Path") { copy(change.path) }
    }

    private var emptyRepository: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("This folder is not a Git repository.").font(.callout).foregroundStyle(.secondary)
            Button("Initialize Repository…") { open("init") }.buttonStyle(.borderedProminent)
            Button("Clone Repository…") { open("clone") }
            Spacer()
        }.frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: Graph pane

    private var history: some View {
        let rows = store.git.history
        let gutter = GitMetrics.gutter(store.git.graph_width)
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text("GRAPH").font(.caption2.bold()).foregroundStyle(.secondary)
                Text("\(rows.count)").font(.caption2.monospacedDigit()).foregroundStyle(.tertiary)
                TextField("Filter", text: $filter).textFieldStyle(.roundedBorder).controlSize(.small)
            }.padding(.horizontal, 10)
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(rows, id: \.commit.id) { row in
                            commitRow(row, gutter: gutter).id(row.commit.id)
                            if selectedCommit == row.commit.id { commitDetail(row, gutter: gutter) }
                        }
                        if rows.count >= store.git.history_limit {
                            Button("Load More Commits") { store.gitLoadMore() }
                                .buttonStyle(.link).font(.caption).padding(8).disabled(busy)
                        }
                        if rows.isEmpty && repo != nil {
                            Text("No commits yet").font(.caption).foregroundStyle(.secondary).padding(10)
                        }
                    }
                }
                .onChange(of: selectedCommit) { _, id in
                    if let id { withAnimation { proxy.scrollTo(id) } }
                }
            }
        }
        .padding(.top, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Source Control Graph")
        .accessibilityIdentifier("gitGraph")
    }

    private func matches(_ row: GitPanelState.Row) -> Bool {
        filter.isEmpty || ([row.commit.subject, row.commit.author, row.commit.id] + row.commit.refs.map(\.name))
            .contains { $0.localizedCaseInsensitiveContains(filter) }
    }

    private func commitRow(_ row: GitPanelState.Row, gutter: CGFloat) -> some View {
        let selected = selectedCommit == row.commit.id
        return GitCommitRow(row: row, gutter: gutter, selected: selected, dimmed: !matches(row),
            refMenu: { ref in AnyView(refMenu(ref)) })
        .onTapGesture { toggle(row.commit.id) }
        .contextMenu { commitMenu(row.commit.id, subject: row.commit.subject) }
    }

    private func toggle(_ id: String) {
        if selectedCommit == id { selectedCommit = nil; return }
        selectedCommit = id
        if store.git.inspection?.commit?.id != id { store.gitAction("show_commit", id) }
    }

    @ViewBuilder private func commitDetail(_ row: GitPanelState.Row, gutter: CGFloat) -> some View {
        HStack(alignment: .top, spacing: 0) {
            GitLaneContinuation(row: row).frame(width: gutter)
            if let detail = store.git.inspection?.commit, detail.id == row.commit.id {
                GitCommitDetailView(detail: detail, refs: row.commit.refs, color: laneColor(row.color),
                    refMenu: { ref in AnyView(refMenu(ref)) },
                    openFile: { file in store.gitAction("show_commit_file", detail.id, file.path); reviewing = true },
                    fullDiff: { inspect("show_commit", detail.id) })
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity).padding(8)
            }
        }
        .padding(.trailing, 8)
    }

    @ViewBuilder private func commitMenu(_ id: String, subject: String) -> some View {
        Button("Show Changes") { inspect("show_commit", id) }
        Divider()
        Button("Checkout (Detached)") { store.gitAction("checkout", id) }
        Button("Create Branch Here…") { open("branch", second: id) }
        Button("Create Tag Here…") { open("tag", second: id) }
        Divider()
        Button("Merge into Current Branch") { store.gitAction("merge", id) }
        Button("Rebase Current Branch onto This…") { confirm { store.gitAction("rebase", id) } }
        Button("Cherry-pick") { store.gitAction("cherry_pick", id) }
        Button("Revert Commit") { store.gitAction("revert", id) }
        Button("Compare with HEAD") { inspect("compare", id, "HEAD") }
        Divider()
        Button("Copy Commit Hash") { copy(id) }
        Button("Copy Subject") { copy(subject) }
    }

    @ViewBuilder private func refMenu(_ ref: GitPanelState.Ref) -> some View {
        switch ref.kind {
        case "branch":
            Button("Checkout \(ref.name)") { store.gitAction("checkout", ref.name) }.disabled(ref.current)
            Button("Merge into Current Branch") { store.gitAction("merge", ref.name) }.disabled(ref.current)
            Button("Rebase Current Branch onto \(ref.name)…") { confirm { store.gitAction("rebase", ref.name) } }.disabled(ref.current)
            Divider()
            Button("Rename…") { open("rename_branch", first: ref.name) }
            Button("Delete…", role: .destructive) { open("delete_branch", first: ref.name) }.disabled(ref.current)
        case "remote":
            Button("Merge into Current Branch") { store.gitAction("merge", ref.name) }
            Button("Rebase Current Branch onto \(ref.name)…") { confirm { store.gitAction("rebase", ref.name) } }
            Button("Create Local Branch…") { open("branch", first: workspacePathName(value: ref.name), second: ref.name) }
        case "tag":
            Button("Checkout (Detached)") { store.gitAction("checkout", ref.name) }
            Button("Delete Tag…", role: .destructive) { confirm { store.gitAction("tag_delete", ref.name) } }
        default:
            Button("Create Branch Here…") { open("branch") }
        }
        Divider()
        Button("Copy Name") { copy(ref.name) }
    }

    // MARK: Forms and helpers

    private func actionForm(_ action: GitPanelState.Action) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(action.title).font(.headline)
            if !action.first.isEmpty { TextField(action.first, text: $first).textFieldStyle(.roundedBorder) }
            if !action.second.isEmpty { TextField(action.second, text: $second).textFieldStyle(.roundedBorder) }
            if !action.option.isEmpty { Toggle(action.option, isOn: $option) }
            if action.destructive {
                Label(workspaceGitConfirmation().body, systemImage: "exclamationmark.triangle")
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Spacer()
                Button("Cancel") { form = nil }.keyboardShortcut(.cancelAction)
                Button(action.title, role: action.destructive ? .destructive : nil) {
                    store.gitAction(action.id, first, second, option); form = nil
                }.keyboardShortcut(.defaultAction)
            }
        }.padding().frame(width: 440)
    }

    private func loadRepositoryIfNeeded() {
        guard let root = store.workspaceRoot, requestedRoot != root, !busy else { return }
        requestedRoot = root
        store.refreshGit()
    }
    private func open(_ id: String, first: String = "", second: String = "") {
        guard let action = store.git.actions.first(where: { $0.id == id }) else { return }
        open(action)
        self.first = first
        self.second = second
    }
    private func open(_ action: GitPanelState.Action) {
        guard !(action.first.isEmpty && action.second.isEmpty && action.option.isEmpty) else {
            if action.destructive { confirm { store.gitAction(action.id) } } else { store.gitAction(action.id) }
            return
        }
        first = ""; second = ""; option = false; form = action
    }
    private func openFile(_ path: String) {
        guard let root = repo?.root else { return }
        store.openPaths([URL(fileURLWithPath: root).appendingPathComponent(path)])
    }
    private func confirm(_ action: @escaping () -> Void) { confirmation = action; confirming = true }
    private func review(_ change: GitChange, staged: Bool) {
        if change.conflicted { inspect("resolve", change.path); return }
        store.showGitDiff(change.path, staged: staged)
        reviewing = true
    }
    private func inspect(_ action: String, _ value: String, _ extra: String = "") {
        store.gitAction(action, value, extra)
        reviewing = true
    }
}

// MARK: - Change rows

private struct GitSectionHeader: View {
    let title: String
    let count: Int
    let open: Bool
    let staged: Bool
    let busy: Bool
    let toggle: () -> Void
    let all: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold))
                .rotationEffect(.degrees(open ? 90 : 0)).frame(width: 12)
            Text(title.uppercased()).font(.caption2.bold())
            Spacer(minLength: 4)
            if hovering && count > 0 {
                Button(staged ? "Unstage All" : "Stage All", systemImage: staged ? "minus" : "plus", action: all)
                    .labelStyle(.iconOnly).buttonStyle(.borderless).help(staged ? "Unstage All" : "Stage All")
                    .disabled(busy)
            }
            Text("\(count)").font(.caption2.monospacedDigit())
                .padding(.horizontal, 5).padding(.vertical, 1)
                .background(.quaternary, in: Capsule())
        }
        .foregroundStyle(.secondary)
        .frame(height: 22)
        .contentShape(Rectangle())
        .onTapGesture(perform: toggle)
        .onHover { hovering = $0 }
    }
}

private struct GitChangeRow: View {
    let change: GitChange
    let staged: Bool
    let busy: Bool
    let review: () -> Void
    let openFile: () -> Void
    let stage: () -> Void
    let discard: () -> Void
    @State private var hovering = false

    var body: some View {
        let (letter, color) = changeStatus(change, staged: staged)
        let directory = workspacePathParent(value: change.path)
        HStack(spacing: 5) {
            if let image = WorkspaceIcons.image(for: change.path) {
                Image(nsImage: image).resizable().frame(width: 14, height: 14)
            }
            Text(workspacePathName(value: change.path))
                .truncationMode(.middle)
                .strikethrough(letter == "D")
                .foregroundStyle(change.conflicted ? .red : .primary)
                .lineLimit(1).layoutPriority(1)
            if !directory.isEmpty {
                Text(directory).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
            }
            Spacer(minLength: 2)
            if hovering {
                Group {
                    Button("Open File", systemImage: "doc.text", action: openFile)
                    if !staged && !change.conflicted {
                        Button("Discard Changes", systemImage: "arrow.uturn.backward", action: discard)
                    }
                    Button(staged ? "Unstage" : "Stage", systemImage: staged ? "minus" : "plus", action: stage)
                }
                .labelStyle(.iconOnly).buttonStyle(.borderless).disabled(busy)
            }
            Text(letter).font(.caption.monospaced().bold()).foregroundStyle(color).frame(width: 12)
        }
        .font(.callout)
        .padding(.leading, 16).padding(.trailing, 2)
        .frame(height: 22)
        .background(hovering ? Color.primary.opacity(0.06) : .clear, in: RoundedRectangle(cornerRadius: 4))
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onTapGesture(count: 2, perform: openFile)
        .onTapGesture(perform: review)
        .help(change.original_path.map { "\($0) → \(change.path)" } ?? change.path)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(change.path), \(letter)")
    }
}

// MARK: - Graph rows

private struct GitGraphCanvas: View {
    let row: GitPanelState.Row

    var body: some View {
        Canvas { context, size in
            let w = GitMetrics.lane
            func x(_ lane: Int) -> CGFloat { CGFloat(lane) * w + w / 2 + 2 }
            for edge in row.edges where max(edge.from, edge.to) < GitMetrics.maxLanes {
                let a = CGPoint(x: x(edge.from), y: CGFloat(edge.start) * size.height / 2)
                let b = CGPoint(x: x(edge.to), y: CGFloat(edge.end) * size.height / 2)
                var path = Path()
                path.move(to: a)
                if a.x == b.x {
                    path.addLine(to: b)
                } else {
                    let mid = (a.y + b.y) / 2
                    path.addCurve(to: b, control1: CGPoint(x: a.x, y: mid), control2: CGPoint(x: b.x, y: mid))
                }
                context.stroke(path, with: .color(laneColor(edge.color)), style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
            }
            guard row.lane < GitMetrics.maxLanes else { return }
            let center = CGPoint(x: x(row.lane), y: size.height / 2)
            let color = laneColor(row.color)
            let radius: CGFloat = row.head ? 4.5 : 3.5
            let dot = Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
            if row.merge {
                context.fill(dot, with: .color(Color(nsColor: .windowBackgroundColor)))
                context.stroke(dot, with: .color(color), lineWidth: 1.8)
            } else {
                context.fill(dot, with: .color(color))
            }
            if row.head {
                let ring = Path(ellipseIn: CGRect(x: center.x - 6.5, y: center.y - 6.5, width: 13, height: 13))
                context.stroke(ring, with: .color(color.opacity(0.55)), lineWidth: 1.2)
            }
        }
    }
}

/// Keeps lanes visually continuous through an expanded commit detail.
private struct GitLaneContinuation: View {
    let row: GitPanelState.Row

    var body: some View {
        Canvas { context, size in
            var drawn = Set<Int>()
            for edge in row.edges where edge.end == 2 && edge.to < GitMetrics.maxLanes && drawn.insert(edge.to).inserted {
                let x = CGFloat(edge.to) * GitMetrics.lane + GitMetrics.lane / 2 + 2
                var path = Path()
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x, y: size.height))
                context.stroke(path, with: .color(laneColor(edge.color)), lineWidth: 1.6)
            }
        }
    }
}

private struct GitRefBadge: View {
    let ref: GitPanelState.Ref
    let color: Color

    var body: some View {
        let (icon, tint): (String, Color) = switch ref.kind {
        case "tag": ("tag.fill", .yellow)
        case "remote": ("cloud", color)
        case "head": ("location.fill", .red)
        default: (ref.current ? "checkmark" : "arrow.triangle.branch", color)
        }
        HStack(spacing: 2) {
            Image(systemName: icon).imageScale(.small)
            Text(ref.name).lineLimit(1).truncationMode(.middle)
        }
        .font(.system(size: 10, weight: ref.current ? .bold : .medium))
        .padding(.horizontal, 5).frame(height: 16)
        .foregroundStyle(ref.current ? Color.white : tint)
        .background(ref.current ? tint : tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
        .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(tint.opacity(ref.kind == "remote" ? 0.6 : 0), lineWidth: 1))
        .help(ref.name)
        .accessibilityIdentifier("gitRef:\(ref.name)")
    }
}

/// Proposes at most `limit` points: short content keeps its natural size, long
/// text truncates instead of widening its row.
private struct WidthLimit: Layout {
    let limit: CGFloat
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = min(proposal.width ?? limit, limit)
        return subviews.first?.sizeThatFits(ProposedViewSize(width: width, height: proposal.height)) ?? .zero
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
    }
}

/// Wraps children onto new lines within the offered width.
private struct GitFlow: Layout {
    var spacing: CGFloat = 4
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let frames = arrange(width: proposal.width ?? .infinity, subviews: subviews)
        return CGSize(width: frames.map(\.maxX).max() ?? 0, height: frames.map(\.maxY).max() ?? 0)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (frame, subview) in zip(arrange(width: bounds.width, subviews: subviews), subviews) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          proposal: ProposedViewSize(frame.size))
        }
    }
    private func arrange(width: CGFloat, subviews: Subviews) -> [CGRect] {
        var frames: [CGRect] = []
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0
        for subview in subviews {
            // A single item wider than the line is narrowed and truncates.
            var size = subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
            size.width = min(size.width, width)
            if x > 0 && x + size.width > width { x = 0; y += line + spacing; line = 0 }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + spacing
            line = max(line, size.height)
        }
        return frames
    }
}

private struct GitCommitRow: View {
    let row: GitPanelState.Row
    let gutter: CGFloat
    let selected: Bool
    let dimmed: Bool
    let refMenu: (GitPanelState.Ref) -> AnyView
    @State private var hovering = false

    var body: some View {
        let commit = row.commit
        HStack(spacing: 5) {
            GitGraphCanvas(row: row).frame(width: gutter, height: GitMetrics.row)
            // One badge in the row; every reference is listed in the commit detail.
            if let ref = commit.refs.first {
                WidthLimit(limit: GitMetrics.badge) {
                    GitRefBadge(ref: ref, color: laneColor(row.color))
                }
                .contextMenu { refMenu(ref) }
            }
            if commit.refs.count > 1 {
                Text("+\(commit.refs.count - 1)")
                    .font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
                    .padding(.horizontal, 4).frame(height: 16)
                    .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 4))
                    .fixedSize()
                    .help(commit.refs.map(\.name).joined(separator: "\n"))
            }
            Text(commit.subject)
                .font(.callout.weight(row.head ? .semibold : .regular))
                .lineLimit(1).truncationMode(.tail)
                .frame(minWidth: 24, alignment: .leading)
            Spacer(minLength: 4)
            Text(shortDate(commit.date))
                .font(.caption.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
                .fixedSize()
        }
        .padding(.trailing, 10)
        .frame(maxWidth: .infinity, minHeight: GitMetrics.row, maxHeight: GitMetrics.row, alignment: .leading)
        .background(selected ? Color.accentColor.opacity(0.18) : hovering ? Color.primary.opacity(0.05) : .clear)
        .opacity(dimmed ? 0.3 : 1)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .help("\(commit.subject)\n\(commit.author) · \(gitDate(commit.date)?.formatted(date: .abbreviated, time: .shortened) ?? commit.date)\n\(commit.id)")
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(commit.subject), \(commit.author), \(row.short_id)")
        .accessibilityIdentifier("gitCommit:\(row.short_id)")
    }
}

private struct GitCommitDetailView: View {
    let detail: GitPanelState.CommitDetail
    let refs: [GitPanelState.Ref]
    let color: Color
    let refMenu: (GitPanelState.Ref) -> AnyView
    let openFile: (GitPanelState.CommitFile) -> Void
    let fullDiff: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            if !refs.isEmpty {
                GitFlow {
                    ForEach(refs, id: \.name) { ref in
                        GitRefBadge(ref: ref, color: color).contextMenu { refMenu(ref) }
                    }
                }
            }
            Text(detail.message).font(.callout).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 6) {
                Text(initials).font(.system(size: 9, weight: .bold)).foregroundStyle(.white)
                    .frame(width: 18, height: 18).background(Circle().fill(.tint))
                VStack(alignment: .leading, spacing: 0) {
                    Text(detail.author).font(.caption.bold()).lineLimit(1).truncationMode(.tail)
                    Text(gitDate(detail.date)?.formatted(date: .abbreviated, time: .shortened) ?? detail.date)
                        .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
                .help(detail.email)
                Spacer(minLength: 4)
                Button { copy(detail.id) } label: {
                    Label(String(detail.id.prefix(8)), systemImage: "doc.on.doc").font(.caption.monospaced())
                }.buttonStyle(.borderless).fixedSize().help("Copy Commit Hash")
            }
            if !detail.files.isEmpty {
                Divider()
                HStack {
                    Text("\(detail.files.count) file\(detail.files.count == 1 ? "" : "s") changed")
                        .font(.caption2.bold()).foregroundStyle(.secondary).lineLimit(1)
                    Spacer()
                    Button("Full Diff", action: fullDiff).buttonStyle(.link).font(.caption2)
                }
                ForEach(detail.files, id: \.path) { file in
                    GitCommitFileRow(file: file).onTapGesture { openFile(file) }
                }
            }
        }
        .padding(8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
        .padding(.vertical, 3)
    }

    private var initials: String {
        String(detail.author.split(separator: " ").prefix(2).compactMap(\.first)).uppercased()
    }
}

private struct GitCommitFileRow: View {
    let file: GitPanelState.CommitFile
    @State private var hovering = false

    var body: some View {
        let (letter, color) = fileStatus(file.status)
        let directory = workspacePathParent(value: file.path)
        HStack(spacing: 5) {
            if let image = WorkspaceIcons.image(for: file.path) { Image(nsImage: image).resizable().frame(width: 13, height: 13) }
            Text(workspacePathName(value: file.path)).lineLimit(1).truncationMode(.middle).strikethrough(letter == "D").layoutPriority(1)
            if !directory.isEmpty { Text(directory).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head) }
            Spacer(minLength: 2)
            Text(letter).font(.caption.monospaced().bold()).foregroundStyle(color)
        }
        .font(.caption)
        .frame(height: 19)
        .padding(.horizontal, 3)
        .background(hovering ? Color.primary.opacity(0.07) : .clear, in: RoundedRectangle(cornerRadius: 3))
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .help(file.original_path.map { "\($0) → \(file.path)" } ?? file.path)
    }
}

// MARK: - Review sheet

private struct GitReviewSheet: View {
    @ObservedObject var store: WorkspaceStore
    let done: () -> Void
    @State private var split = false
    @State private var selected: Set<Int> = []
    @State private var mergeDraft = ""
    @State private var conflictKey = ""

    private var git: GitPanelState { store.git }
    private var busy: Bool { store.backendBusy }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
            if git.diff?.truncated == true || git.inspection?.truncated == true {
                Divider()
                Label("Output truncated at the inspection limit.", systemImage: "scissors")
                    .font(.caption).foregroundStyle(.secondary).padding(6)
            }
        }
        .frame(minWidth: 860, minHeight: 560)
        .onChange(of: git.diff?.text) { _, _ in selected = [] }
    }

    private var title: String { git.inspection?.title ?? git.diff?.path ?? "Git Changes" }
    private var subtitle: String {
        if let diff = git.diff { return diff.staged ? "Staged changes (index ↔ HEAD)" : "Working tree changes (file ↔ index)" }
        if let commit = git.inspection?.commit { return "\(commit.author) · \(commit.id.prefix(10))" }
        return ""
    }

    private var header: some View {
        HStack(spacing: 10) {
            if let path = git.diff?.path ?? git.inspection?.path, let image = WorkspaceIcons.image(for: path) {
                Image(nsImage: image).resizable().frame(width: 18, height: 18)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.headline).lineLimit(1).truncationMode(.middle)
                if !subtitle.isEmpty { Text(subtitle).font(.caption).foregroundStyle(.secondary) }
            }
            Spacer()
            if busy { ProgressView().controlSize(.small) }
            if let diff = git.diff {
                if diff.before != nil && diff.after != nil {
                    Picker("View", selection: $split) {
                        Text("Unified").tag(false)
                        Text("Side by Side").tag(true)
                    }.pickerStyle(.segmented).labelsHidden().fixedSize()
                }
                if !split {
                    Button(diff.staged ? "Unstage \(selected.count) Lines" : "Stage \(selected.count) Lines") {
                        store.gitStageLines(selected); done()
                    }
                    .disabled(selected.isEmpty || busy)
                    .help("Select changed lines or click a hunk header")
                }
            }
            Button("Done", action: done).keyboardShortcut(.cancelAction)
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }

    @ViewBuilder private var content: some View {
        if let error = store.errorMessage {
            Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(.red).textSelection(.enabled).padding()
        } else if let conflict = git.inspection?.conflict {
            mergeEditor(conflict)
        } else if split, let before = git.diff?.before, let after = git.diff?.after {
            HSplitView {
                pane("Before", before)
                pane("After", after)
            }
        } else if !git.patch_lines.isEmpty {
            unified
        } else if busy {
            ProgressView("Loading…")
        } else {
            ScrollView([.horizontal, .vertical]) {
                Text(git.inspection?.text ?? git.diff?.text ?? "").font(.system(size: 11, design: .monospaced))
                    .textSelection(.enabled).padding(8).frame(maxWidth: .infinity, alignment: .topLeading)
            }
        }
    }

    private func pane(_ title: String, _ text: String) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title).font(.caption.bold()).foregroundStyle(.secondary).padding(6)
            Divider()
            ScrollView([.horizontal, .vertical]) {
                Text(text).font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
                    .padding(8).frame(maxWidth: .infinity, alignment: .topLeading)
            }
        }
    }

    private var unified: some View {
        let staging = git.diff != nil
        let lines = git.patch_lines
        return ScrollView([.vertical, .horizontal]) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(lines, id: \.index) { line in
                    GitPatchRow(line: line, staging: staging, selected: selected.contains(line.index),
                        toggle: { toggle(line, in: lines) })
                }
            }
            .padding(.vertical, 4)
            .textSelection(.enabled)
        }
    }

    /// Hunk headers toggle every changed line in their hunk.
    private func toggle(_ line: GitPanelState.PatchLine, in lines: [GitPanelState.PatchLine]) {
        guard git.diff != nil else { return }
        var targets = [line.index]
        if line.kind == "hunk", let start = lines.firstIndex(where: { $0.index == line.index }) {
            targets = lines[(start + 1)...].prefix { $0.kind != "hunk" && $0.kind != "meta" }.filter(\.changed).map(\.index)
        }
        if targets.allSatisfy(selected.contains) { selected.subtract(targets) } else { selected.formUnion(targets) }
    }

    private func mergeEditor(_ conflict: GitPanelState.Conflict) -> some View {
        VStack(spacing: 8) {
            HSplitView {
                pane("Current (ours)", conflict.current)
                pane("Incoming (theirs)", conflict.incoming)
            }.frame(minHeight: 160, idealHeight: 200)
            HStack {
                Button("Accept Current") { mergeDraft = conflict.accept_current }
                Button("Accept Incoming") { mergeDraft = conflict.accept_incoming }
                Button("Accept Both") { mergeDraft = conflict.accept_both }
                Button("Reset") { mergeDraft = conflict.expected }
                Spacer()
                Button("Save and Stage Result") { store.gitResolve(mergeDraft); done() }
                    .buttonStyle(.borderedProminent).disabled(busy)
            }.padding(.horizontal, 10)
            TextEditor(text: $mergeDraft).font(.system(size: 12, design: .monospaced))
                .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(.separator))
                .padding([.horizontal, .bottom], 10)
        }
        .padding(.top, 8)
        .onAppear { load(conflict) }
        .onChange(of: conflict) { _, value in load(value) }
    }

    private func load(_ conflict: GitPanelState.Conflict) {
        let key = conflict.path + conflict.expected
        if key != conflictKey { conflictKey = key; mergeDraft = conflict.expected }
    }
}

private struct GitPatchRow: View {
    let line: GitPanelState.PatchLine
    let staging: Bool
    let selected: Bool
    let toggle: () -> Void

    var body: some View {
        let text = line.text.trimmingCharacters(in: .newlines)
        let file = line.kind == "meta" && text.hasPrefix("diff --git ")
        HStack(spacing: 0) {
            if staging {
                Group {
                    if line.changed {
                        Image(systemName: selected ? "checkmark.square.fill" : "square")
                            .foregroundStyle(selected ? Color.accentColor : .secondary)
                    } else { Color.clear }
                }
                .frame(width: 20)
                .contentShape(Rectangle())
                .onTapGesture(perform: toggle)
            }
            number(line.old)
            number(line.new)
            Text(text.isEmpty ? " " : text)
                .font(.system(size: 11, weight: file ? .bold : .regular, design: .monospaced))
                .foregroundStyle(foreground)
                .padding(.leading, 6)
                .fixedSize()
            Spacer(minLength: 0)
        }
        .frame(minHeight: 17)
        .background(background(file: file))
        .contentShape(Rectangle())
        .onTapGesture { if staging && (line.changed || line.kind == "hunk") { toggle() } }
    }

    private func number(_ value: Int?) -> some View {
        Text(value.map(String.init) ?? "")
            .font(.system(size: 10, design: .monospaced)).foregroundStyle(.tertiary)
            .frame(width: 38, alignment: .trailing)
    }

    private var foreground: Color {
        switch line.kind {
        case "added": Color(nsColor: .systemGreen)
        case "removed": Color(nsColor: .systemRed)
        case "hunk": Color(nsColor: .systemBlue)
        case "meta": .secondary
        default: .primary
        }
    }

    private func background(file: Bool) -> Color {
        if selected { return Color.accentColor.opacity(0.22) }
        switch line.kind {
        case "added": return Color.green.opacity(0.12)
        case "removed": return Color.red.opacity(0.12)
        case "hunk": return Color.blue.opacity(0.08)
        default: return file ? Color.primary.opacity(0.07) : .clear
        }
    }
}
