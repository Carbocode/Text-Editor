import AppKit
import SwiftUI
import Combine

/// AppKit may deliver launch files before SwiftUI creates either window.
@MainActor
final class WorkspaceAppDelegate: NSObject, NSApplicationDelegate, ObservableObject {
    let store: WorkspaceStore
    private var presentWorkspace: (() -> Void)?
    private var pendingPresentation = false
    private var receivedOpenRequest = false
    private var restorePresentation: AnyCancellable?
    private var confirmingQuit = false

    override init() { store = WorkspaceStore(); super.init() }
    init(store: WorkspaceStore) { self.store = store; super.init() }

    func application(_ application: NSApplication, open urls: [URL]) {
        guard !urls.isEmpty else { return }
        receivedOpenRequest = true
        store.openPaths(urls)
        pendingPresentation = true
        showPendingWorkspace()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard store.persistenceEnabled, !receivedOpenRequest else { return }
        restorePresentation = store.$workspaceRoot.compactMap { $0 }.prefix(1).sink { [weak self] _ in
            self?.pendingPresentation = true
            self?.showPendingWorkspace()
        }
        store.restoreSession()
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !confirmingQuit else { return .terminateCancel }
        if store.backendBusy {
            store.prepareToQuit(save: false)
            return .terminateCancel
        }
        guard store.documents.contains(where: \.dirty) else {
            return finishQuit(sender, save: false)
        }
        confirmingQuit = true
        defer { confirmingQuit = false }
        let prompt = workspaceQuitConfirmation()
        let alert = NSAlert()
        alert.messageText = prompt.title
        alert.informativeText = prompt.body
        alert.addButton(withTitle: prompt.confirm)
        alert.addButton(withTitle: prompt.cancel)
        alert.addButton(withTitle: prompt.discard)
        switch alert.runModal() {
        case .alertFirstButtonReturn:
            return finishQuit(sender, save: true)
        case .alertThirdButtonReturn:
            return finishQuit(sender, save: false)
        default: return .terminateCancel
        }
    }

    private func finishQuit(_ sender: NSApplication, save: Bool) -> NSApplication.TerminateReply {
        guard store.prepareToQuit(save: save) else { return .terminateCancel }
        guard store.terminalsRunning else { return .terminateNow }
        confirmingQuit = true
        defer { confirmingQuit = false }
        let prompt = workspaceTerminalConfirmation()
        let alert = NSAlert()
        alert.messageText = prompt.title
        alert.informativeText = prompt.body
        alert.addButton(withTitle: prompt.confirm)
        alert.addButton(withTitle: prompt.cancel)
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        store.stopTerminalsForQuit { sender.reply(toApplicationShouldTerminate: true) }
        return .terminateLater
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
    @Environment(\.dismissWindow) private var dismissWindow

    func body(content: Content) -> some View {
        content.onAppear {
            delegate.installPresentation {
                openWindow(id: "workspace")
                dismissWindow(id: "home")
            }
        }
    }
}
