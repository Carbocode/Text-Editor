import AppKit
import SwiftUI

/// AppKit may deliver launch files before SwiftUI creates either window.
@MainActor
final class WorkspaceAppDelegate: NSObject, NSApplicationDelegate, ObservableObject {
    let store: WorkspaceStore
    private var presentWorkspace: (() -> Void)?
    private var pendingPresentation = false

    override init() { store = WorkspaceStore(); super.init() }
    init(store: WorkspaceStore) { self.store = store; super.init() }

    func application(_ application: NSApplication, open urls: [URL]) {
        guard !urls.isEmpty else { return }
        store.openPaths(urls)
        pendingPresentation = true
        showPendingWorkspace()
    }

    func installPresentation(_ present: @escaping () -> Void) {
        presentWorkspace = present
        showPendingWorkspace()
    }

    private func showPendingWorkspace() {
        guard pendingPresentation, let presentWorkspace else { return }
        pendingPresentation = false
        presentWorkspace()
        NSApp.activate(ignoringOtherApps: true)
    }
}

struct SystemOpenPresentation: ViewModifier {
    let delegate: WorkspaceAppDelegate
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content.onAppear {
            delegate.installPresentation { openWindow(id: "workspace") }
        }
    }
}
