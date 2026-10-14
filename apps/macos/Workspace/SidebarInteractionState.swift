import AppKit
import Combine
import SwiftUI

/// Hover publications are observed only by the small drop decorations, not the entire workspace.
@MainActor
final class SidebarDragState: ObservableObject {
    @Published private(set) var directory: String?
    @Published private(set) var valid = false
    @Published private(set) var source: String?
    private var hoverToken = UUID()
    private var leaveToken = UUID()
    private var temporaryFolders: [String: () -> Void] = [:]
    private var dragWatchdog: Timer?
    private var releasedAt: Date?
    private var dismissPreview: (() -> Void)?
    private let isDragging: () -> Bool
    private let expansionDelay: TimeInterval

    init(expansionDelay: TimeInterval = 0.45,
         isDragging: @escaping () -> Bool = { NSEvent.pressedMouseButtons & 1 != 0 }) {
        self.expansionDelay = expansionDelay
        self.isDragging = isDragging
    }

    func begin(_ path: String) {
        finish()
        source = path
        let timer = Timer(timeInterval: 0.1, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated {
                guard let self, self.source != nil else { timer.invalidate(); return }
                if self.isDragging() { self.releasedAt = nil }
                else if let releasedAt = self.releasedAt {
                    if Date().timeIntervalSince(releasedAt) >= 0.15 { self.finish() }
                } else { self.releasedAt = Date() }
            }
        }
        dragWatchdog = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    /// Enter/motion share a folder deadline. Native exit/enter pairs caused by
    /// child hit testing have a short grace period and do not restart that deadline.
    func enter(_ target: String, valid: Bool, expand: @escaping () -> Bool,
               collapse: @escaping () -> Void = {}) {
        guard source != nil else { return }
        leaveToken = UUID()
        if directory == target { updateValidity(valid); return }
        restoreTemporaryFolders(exceptAncestorsOf: target)
        directory = target
        updateValidity(valid)
        let generation = UUID()
        hoverToken = generation
        DispatchQueue.main.asyncAfter(deadline: .now() + expansionDelay) { [weak self] in
            guard let self, self.hoverToken == generation, self.directory == target,
                  self.source != nil, self.valid, self.isDragging() else { return }
            if expand() { self.temporaryFolders[target] = collapse }
        }
    }

    func updateValidity(_ value: Bool) {
        let activeValue = source != nil && value
        if valid != activeValue { valid = activeValue }
    }

    func leave(_ target: String) {
        guard directory == target else { return }
        let generation = UUID()
        leaveToken = generation
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            guard let self, self.leaveToken == generation, self.directory == target else { return }
            self.directory = nil
            self.hoverToken = UUID()
        }
    }

    private func restoreTemporaryFolders(exceptAncestorsOf destination: String?) {
        let closing = temporaryFolders.keys.filter { folder in
            guard let destination else { return true }
            return destination != folder && !destination.hasPrefix(folder + "/")
        }.sorted { $0.count > $1.count }
        for folder in closing {
            let collapse = temporaryFolders.removeValue(forKey: folder)
            collapse?()
        }
    }

    func setPreviewDismissal(_ dismiss: @escaping () -> Void) { dismissPreview = dismiss }

    func hidePreview() {
        dismissPreview?()
        dismissPreview = nil
    }

    func finish(committing destination: String? = nil) {
        hidePreview()
        dragWatchdog?.invalidate()
        dragWatchdog = nil
        releasedAt = nil
        source = nil
        hoverToken = UUID()
        leaveToken = UUID()
        if directory != nil { directory = nil }
        updateValidity(false)
        restoreTemporaryFolders(exceptAncestorsOf: destination)
        temporaryFolders.removeAll()
    }
}

@MainActor
final class SidebarRenameDraft: ObservableObject {
    @Published private(set) var previewPath: String?
    private(set) var path: String?
    private(set) var name = ""
    private var previewJob: DispatchWorkItem?

    func begin(_ path: String) {
        previewJob?.cancel()
        self.path = path
        name = path.split(separator: "/").last.map(String.init) ?? path
        previewPath = path
    }

    func changeName(_ name: String) {
        self.name = name
        previewJob?.cancel()
        guard let path else { return }
        let job = DispatchWorkItem { [weak self] in
            guard let self, self.path == path else { return }
            let parent = path.split(separator: "/").dropLast().joined(separator: "/")
            self.previewPath = parent.isEmpty ? name : parent + "/" + name
        }
        previewJob = job
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15, execute: job)
    }

    func finish() {
        previewJob?.cancel()
        previewJob = nil
        path = nil
        if previewPath != nil { previewPath = nil }
    }
}

struct SidebarEntryIcon: View {
    @ObservedObject var draft: SidebarRenameDraft
    let originalPath: String
    let displayPath: String
    var directory = false
    var expanded = false

    var body: some View {
        let path = draft.path == originalPath ? (draft.previewPath ?? displayPath) : displayPath
        if let image = WorkspaceIcons.image(for: path, isDirectory: directory, expanded: expanded) {
            Image(nsImage: image).resizable().interpolation(.high).frame(width: 16, height: 16)
        }
    }
}

struct SidebarRowHighlight: View {
    var color: Color = .accentColor
    var cornerRadius: CGFloat = 4
    var outlined = false

    var body: some View {
        RoundedRectangle(cornerRadius: cornerRadius)
            .fill(color.opacity(0.14))
            .overlay {
                if outlined {
                    RoundedRectangle(cornerRadius: cornerRadius)
                        .stroke(color, lineWidth: 2)
                }
            }
            .allowsHitTesting(false)
    }
}

struct SidebarDropOutline: View {
    @ObservedObject var drag: SidebarDragState
    let directory: String
    var cornerRadius: CGFloat = 4
    var selected = false

    var body: some View {
        let targeted = drag.source != nil && drag.directory == directory
        if targeted || selected {
            SidebarRowHighlight(color: targeted && !drag.valid ? .red : .accentColor,
                                cornerRadius: cornerRadius, outlined: targeted)
        }
    }
}

struct SidebarRootDropLabel: View {
    @ObservedObject var drag: SidebarDragState
    let workspaceName: String

    var body: some View {
        if drag.source != nil {
            Label("Move to workspace root", systemImage: "folder")
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 32)
                .glassEffect(.regular, in: RoundedRectangle(cornerRadius: 10))
                .overlay { SidebarDropOutline(drag: drag, directory: "", cornerRadius: 10) }
                .contentShape(Rectangle())
                .help("Move a file or folder to the root of " + workspaceName)
                .padding(.horizontal, 10)
                .padding(.bottom, 8)
        }
    }
}
