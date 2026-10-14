import AppKit
import SwiftUI
import UniformTypeIdentifiers

@MainActor final class Result { var began = false; var dropped = false; var ended = false; var clicked = false }
struct NativeDragHarness: View {
    let result: Result
    let state: SidebarDragState
    let type = UTType(exportedAs: "org.nativeworkspace.sidebar-entry", conformingTo: .data)
    var body: some View {
        HStack(spacing: 0) {
            Button { result.clicked = true } label: {
                Text("source").frame(width: 200, height: 160).contentShape(Rectangle())
            }
            .buttonStyle(.plain).background(Color.gray)
                .background(SidebarNativeDragSource(path: "source", directory: false,
                    type: NSPasteboard.PasteboardType(type.identifier), enabled: true, state: state,
                    begin: { result.began = true; state.begin("source") }, end: { result.ended = true }))
            Text("destination").frame(width: 200, height: 160).background(Color.blue)
                .overlay(SidebarNativeDropTarget(type: NSPasteboard.PasteboardType(type.identifier),
                    state: state, canDrop: { state.source != nil }, entered: {}, exited: {}, dropped: {
                    result.dropped = true
                    state.hidePreview()
                    state.finish()
                    return true
                }))
        }
    }
}
@main struct Smoke {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let result = Result()
        let state = SidebarDragState(isDragging: { true })
        let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 400, height: 160), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: NativeDragHarness(result: result, state: state))
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        func send(_ entries: [(NSEvent.EventType, Double)]) {
            for (index, entry) in entries.enumerated() {
                let event = NSEvent.mouseEvent(with: entry.0, location: NSPoint(x: entry.1, y: 80), modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime + Double(index) * 0.05, windowNumber: window.windowNumber, context: nil, eventNumber: index, clickCount: 1, pressure: entry.0 == .leftMouseUp ? 0 : 1)!
                app.postEvent(event, atStart: false)
            }
            let until = Date().addingTimeInterval(1)
            while Date() < until {
                if let event = app.nextEvent(matching: .any, until: Date().addingTimeInterval(0.05), inMode: .default, dequeue: true) { app.sendEvent(event) }
                RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            }
        }
        send([(.leftMouseDown, 70), (.leftMouseDragged, 85), (.leftMouseDragged, 320), (.leftMouseUp, 320)])
        print("native source began=\(result.began) dropped=\(result.dropped) ended=\(result.ended)")
        precondition(result.began && result.dropped && result.ended)
        precondition(!result.clicked, "Dragging must not activate the source button")
        precondition(state.source == nil, "Native drag end retained source feedback")
        result.began = false
        result.dropped = false
        result.ended = false
        send([(.leftMouseDown, 70), (.leftMouseDragged, 85), (.leftMouseDragged, 450), (.leftMouseUp, 450)])
        precondition(result.began && result.ended && !result.dropped, "Rejected native drag did not finish cleanly")
        precondition(state.source == nil && !result.clicked, "Rejected drag kept feedback or activated the source button")
        send([(.leftMouseDown, 70), (.leftMouseUp, 70)])
        precondition(result.clicked, "Native drag source intercepted a normal click")
        print("macOS native drag/drop/cancel/click cleanup smoke test passed")
    }
}
