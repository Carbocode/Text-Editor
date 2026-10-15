import AppKit
import SwiftUI

@main
struct SidebarInteractionSmoke {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        var value = "hello 💡.rs"
        var submitted = 0
        var cancelled = 0
        let binding = Binding<String>(get: { value }, set: { value = $0 })
        let field = SidebarInlineRename(name: binding, directory: false,
                                        submit: { submitted += 1 }, cancel: { cancelled += 1 })
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 280, height: 100),
                              styleMask: [.titled], backing: .buffered, defer: false)
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 280, height: 100))
        let host = NSHostingView(rootView: field.padding(8))
        host.frame = NSRect(x: 0, y: 50, width: 280, height: 50)
        let clickTarget = RenameClickTarget(frame: NSRect(x: 0, y: 0, width: 280, height: 40))
        content.addSubview(host)
        content.addSubview(clickTarget)
        window.contentView = content
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        guard let editor = window.firstResponder as? NSTextView else {
            preconditionFailure("Inline rename did not focus the native field editor")
        }
        precondition(editor.selectedRange() == NSRange(location: 0, length: ("hello 💡" as NSString).length))
        editor.insertText("renamed", replacementRange: editor.selectedRange())
        precondition(value == "renamed.rs", "Typing should preserve the extension")
        editor.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        precondition(submitted == 1, "Return did not submit rename")
        editor.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        precondition(cancelled == 1, "Escape did not cancel rename")
        let insideUp = NSEvent.mouseEvent(with: .leftMouseUp, location: NSPoint(x: 140, y: 75), modifierFlags: [],
                                          timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                          context: nil, eventNumber: 0, clickCount: 1, pressure: 0)!
        let insideDown = NSEvent.mouseEvent(with: .leftMouseDown, location: NSPoint(x: 140, y: 75), modifierFlags: [],
                                            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                            context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
        app.postEvent(insideUp, atStart: false)
        app.postEvent(insideDown, atStart: true)
        if let event = app.nextEvent(matching: .leftMouseDown, until: Date().addingTimeInterval(0.1),
                                     inMode: .default, dequeue: true) { app.sendEvent(event) }
        precondition(cancelled == 1, "Clicking inside the input cancelled rename")
        for type in [NSEvent.EventType.leftMouseDown, .rightMouseDown, .otherMouseDown] {
            let click = NSEvent.mouseEvent(with: type, location: NSPoint(x: 140, y: 20), modifierFlags: [],
                                          timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                          context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
            app.postEvent(click, atStart: true)
            if let event = app.nextEvent(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown],
                                         until: Date().addingTimeInterval(0.1), inMode: .default, dequeue: true) {
                app.sendEvent(event)
            }
            precondition(clickTarget.received.last == type, "Rename swallowed a click intended for another view")
            precondition(submitted == 1, "Outside click submitted a rename")
        }
        precondition(window.firstResponder === clickTarget, "Outside click did not focus its native target")
        precondition(cancelled == 2, "Native focus loss did not cancel rename")
        print("macOS rename uses native focus loss and preserves outside mouse events")
        let drag = SidebarDragState(isDragging: { true })
        let renders = RenderCounter()
        window.contentView = NSHostingView(rootView: DragRenderHarness(drag: drag, renders: renders))
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        let initialRenders = renders.count
        drag.begin("source.rs")
        for index in 0..<20 {
            drag.enter("folder-\(index)", valid: true, expand: { false })
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        }
        precondition(renders.count == initialRenders, "Drag hover rebuilt the parent workspace view")
        drag.finish()
        precondition(drag.source == nil && drag.directory == nil && !drag.valid)
        for destination in ["", "folder"] {
            drag.begin("source.rs")
            drag.enter(destination, valid: true, expand: { false })
            var dismissed = false
            drag.setPreviewDismissal { dismissed = true }
            drag.hidePreview()
            precondition(dismissed && drag.source != nil, "Release must hide the native image without losing the drop payload")
            drag.finish(committing: destination)
            // AppKit can deliver hover callbacks after performDrop has finished.
            drag.enter(destination, valid: false, expand: { false })
            drag.updateValidity(true)
            precondition(drag.source == nil && drag.directory == nil && !drag.valid,
                         "Late hover restored drop feedback after release")
        }
        let dropBox = NSHostingView(rootView: SidebarRootDropLabel(drag: drag, workspaceName: "fixture"))
        precondition(dropBox.fittingSize.height == 0, "Inactive drop box reserves sidebar space")
        drag.begin("source.rs")
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        precondition(dropBox.fittingSize.height >= 32, "Active drag did not show the drop box")
        drag.finish()
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        precondition(dropBox.fittingSize.height == 0, "Finished drag did not hide the drop box")
        var mouseDown = true
        let cancelledDrag = SidebarDragState(isDragging: { mouseDown })
        cancelledDrag.begin("source.rs")
        cancelledDrag.enter("", valid: false, expand: { false })
        mouseDown = false
        RunLoop.main.run(until: Date().addingTimeInterval(0.4))
        precondition(cancelledDrag.source == nil && cancelledDrag.directory == nil && !cancelledDrag.valid,
                     "Release outside a drop target retained drag feedback")
        print("macOS drop box visibility/late hover/cancel cleanup smoke test passed")
        let hover = SidebarDragState(expansionDelay: 0.2, isDragging: { true })
        var open: Set<String> = ["already-open"]
        func over(_ folder: String) {
            hover.enter(folder, valid: true, expand: {
                if open.contains(folder) { return false }
                open.insert(folder)
                return true
            }, collapse: { open.remove(folder) })
        }
        hover.begin("source.rs")
        over("already-open")
        RunLoop.main.run(until: Date().addingTimeInterval(0.22))
        over("a")
        RunLoop.main.run(until: Date().addingTimeInterval(0.12))
        hover.leave("a")
        over("a")
        RunLoop.main.run(until: Date().addingTimeInterval(0.12))
        precondition(open.contains("a"), "Micro-movement restarted the expansion deadline")
        over("a/child")
        RunLoop.main.run(until: Date().addingTimeInterval(0.22))
        precondition(open.contains("a") && open.contains("a/child"))
        over("b")
        precondition(!open.contains("a") && !open.contains("a/child"), "Sibling hover did not close temporary ancestors")
        RunLoop.main.run(until: Date().addingTimeInterval(0.22))
        hover.finish(committing: "b")
        precondition(open.contains("b") && open.contains("already-open"))
        hover.begin("source.rs")
        over("cancelled")
        RunLoop.main.run(until: Date().addingTimeInterval(0.22))
        hover.finish()
        precondition(!open.contains("cancelled") && open.contains("b") && open.contains("already-open"))
        print("macOS drag hover deadline/sibling rollback/drop promotion smoke test passed")
        window.orderOut(nil)
        print("macOS drag hover isolates decoration updates from the parent view")
        print("macOS inline rename focus/stem/Return/Escape smoke test passed")
    }
}

@MainActor private final class RenameClickTarget: NSView {
    var received: [NSEvent.EventType] = []
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) {
        received.append(event.type)
        window?.makeFirstResponder(self)
    }
    override func rightMouseDown(with event: NSEvent) { received.append(event.type) }
    override func otherMouseDown(with event: NSEvent) { received.append(event.type) }
}

@MainActor private final class RenderCounter { var count = 0 }

private struct DragRenderHarness: View {
    @State var drag: SidebarDragState
    let renders: RenderCounter
    var body: some View {
        renders.count += 1
        return Text("Workspace")
            .frame(width: 260, height: 40)
            .overlay { SidebarDropOutline(drag: drag, directory: "folder-1") }
    }
}
