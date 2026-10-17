import AppKit
import SwiftUI

@main
struct TerminalSmoke {
    @MainActor static func main() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = WorkspaceStore(persistence: false)
        _ = store.openPaths([root])
        func wait(_ condition: () -> Bool) {
            let end = Date().addingTimeInterval(8)
            while !condition() {
                precondition(Date() < end, "Terminal smoke timed out: \(store.errorMessage ?? "")")
                store.poll()
                RunLoop.current.run(until: Date().addingTimeInterval(0.01))
            }
        }
        wait { store.workspaceRoot != nil && !store.backendBusy }
        store.terminalAction("new")
        wait { store.terminals.frame != nil }
        let first = store.terminals.active!
        let host = NSHostingView(rootView: TerminalPanel(store: store))
        let window = NSWindow(contentRect: NSRect(x: 100,y: 100,width: 720,height: 320),styleMask: [.titled,.resizable],backing: .buffered,defer: false)
        window.contentView = host
        window.makeKeyAndOrderFront(nil)
        func surface(_ root: NSView) -> NSView? {
            if root is NSTextInputClient { return root }
            for child in root.subviews { if let found=surface(child) {return found} }
            return nil
        }
        wait { surface(host) != nil }
        let view = surface(host)!
        (view as! NSTextInputClient).insertText("printf 'NATIVE_%s\\n' TERMINAL",replacementRange: NSRange(location: NSNotFound,length: 0))
        let enter = NSEvent.keyEvent(with: .keyDown,location: .zero,modifierFlags: [],timestamp: 0,windowNumber: window.windowNumber,context: nil,characters: "\r",charactersIgnoringModifiers: "\r",isARepeat: false,keyCode: 36)!
        view.keyDown(with: enter)
        wait { store.terminals.frame?.cells.map(\.text).joined().contains("NATIVE_TERMINAL") == true }
        store.terminalAction("new")
        wait { store.terminals.tabs.count == 2 && store.terminals.active != first }
        let second=store.terminals.active!
        store.terminalAction("activate",id: first)
        wait { store.terminals.active == first }
        precondition(store.terminals.frame!.cells.map(\.text).joined().contains("NATIVE_TERMINAL"))
        window.setContentSize(NSSize(width: 500,height: 250))
        wait { (store.terminals.frame?.cols ?? 100) < 70 }
        store.terminalAction("close",id: second,confirmed: false)
        precondition(store.terminals.tabs.count == 2)
        store.terminalAction("close",id: second,confirmed: true)
        wait { store.terminals.tabs.count == 1 }
        store.terminalAction("close_all",confirmed: true)
        wait { !store.terminalsRunning }
        window.orderOut(nil)
        print("macOS native terminal input/render/resize/tab isolation/termination smoke passed")
    }
}
