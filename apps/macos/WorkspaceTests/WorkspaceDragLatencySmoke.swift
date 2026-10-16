import AppKit
import Combine
import CoreGraphics
import Darwin
import Foundation
import SwiftUI

// The native pointer smoke covers AppKit session dispatch. This test drives the
// destination callbacks with a local session double so slow-backend verification
// remains deterministic without changing the system pointer or requiring access.
@MainActor private final class LocalDragInfo: NSObject, NSDraggingInfo {
    let draggingDestinationWindow: NSWindow?
    let draggingSource: Any?
    let draggingLocation: NSPoint
    let draggingSourceOperationMask: NSDragOperation = .move
    let draggedImageLocation: NSPoint = .zero
    let draggedImage: NSImage? = nil
    let draggingPasteboard = NSPasteboard(name: .drag)
    let draggingSequenceNumber = 1
    var draggingFormation: NSDraggingFormation = .none
    var animatesToDestination = true
    var numberOfValidItemsForDrop = 1
    let springLoadingHighlight: NSSpringLoadingHighlight = .none
    init(window: NSWindow, source: Any, point: NSPoint) {
        draggingDestinationWindow = window
        draggingSource = source
        draggingLocation = point
    }
    func slideDraggedImage(to screenPoint: NSPoint) {}
    nonisolated override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }
    func enumerateDraggingItems(options enumOpts: NSDraggingItemEnumerationOptions, for view: NSView?,
        classes classArray: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any],
        using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {}
    func resetSpringLoading() {}
}

@main
struct WorkspaceDragLatencySmoke {
    @MainActor static func main() {
        setbuf(stdout, nil)
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let launch = Timer(timeInterval: 0.1, repeats: false) { _ in
            MainActor.assumeIsolated {
                do { try runSmoke(app); exit(0) }
                catch { print("Drag latency smoke failed: \(error)"); exit(1) }
            }
        }
        RunLoop.main.add(launch, forMode: .common)
        app.run()
    }

    @MainActor static func runSmoke(_ app: NSApplication) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("workspace-drag-latency-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root.appendingPathComponent("src"), withIntermediateDirectories: true)
        try "source\n".write(to: root.appendingPathComponent("README.md"), atomically: true, encoding: .utf8)
        try "notes\n".write(to: root.appendingPathComponent("notes.md"), atomically: true, encoding: .utf8)
        try "fn main() {}\n".write(to: root.appendingPathComponent("src/main.rs"), atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: root) }
        let queue = DispatchQueue(label: "workspace.drag-latency.backend")
        let store = WorkspaceStore(operationQueue: queue, persistence: false)
        store.openWorkspace(root)
        let window = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 1000, height: 700),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: ContentView(store: store))
        window.makeKeyAndOrderFront(nil)
        window.orderFrontRegardless()
        app.activate(ignoringOtherApps: true)
        defer { window.orderOut(nil) }

        func pump(until condition: () -> Bool, timeout: TimeInterval) {
            let deadline = ProcessInfo.processInfo.systemUptime + timeout
            while !condition() && ProcessInfo.processInfo.systemUptime < deadline {
                if let event = app.nextEvent(matching: .any, until: Date().addingTimeInterval(0.005),
                    inMode: .default, dequeue: true) { app.sendEvent(event) }
                RunLoop.main.run(until: Date().addingTimeInterval(0.002))
            }
        }
        func sourceView(_ path: String, in view: NSView?) -> SidebarNativeDragSource.DragView? {
            guard let view else { return nil }
            if let source = view as? SidebarNativeDragSource.DragView,
                source.configuration?.path == path, source.window === window,
                !source.visibleRect.isEmpty { return source }
            for child in view.subviews {
                if let source = sourceView(path, in: child) { return source }
            }
            return nil
        }
        pump(until: { sourceView("README.md", in: window.contentView) != nil
            && sourceView("src", in: window.contentView) != nil }, timeout: 3)
        window.contentView?.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        guard let source = sourceView("README.md", in: window.contentView),
              let destination = sourceView("src", in: window.contentView),
              let state = source.configuration?.state else {
            fatalError("Actual workspace sidebar did not expose the source and destination rows")
        }
        store.openDocument("README.md")
        func editorContainer(in view: NSView?) -> EditorContainer? {
            guard let view else { return nil }
            if let editor = view as? EditorContainer { return editor }
            return view.subviews.lazy.compactMap { editorContainer(in: $0) }.first
        }
        pump(until: { editorContainer(in: window.contentView) != nil }, timeout: 3)
        window.contentView?.layoutSubtreeIfNeeded()
        let editorHeight = editorContainer(in: window.contentView)!.bounds.height
        precondition(editorHeight > window.contentView!.bounds.height * 0.7,
            "Document tabs consume editor space: editor height \(editorHeight), window height \(window.contentView!.bounds.height)")
        let originalEditor = editorContainer(in: window.contentView)!
        func editorReady(_ path: String) -> Bool {
            originalEditor.fileName == path
                && (originalEditor.textView?.delegate as? NoWrapTextEditor.Coordinator)?.documentIdentity == store.editorDocumentIdentity
        }
        func tabPoint(_ index: Int, close: Bool = false) -> NSPoint {
            let editorFrame = originalEditor.convert(originalEditor.bounds, to: nil)
            return NSPoint(x: editorFrame.minX + BonsplitTabBarMetrics.barPadding
                + CGFloat(index) * (BonsplitTabBarMetrics.tabMinWidth + BonsplitTabBarMetrics.tabSpacing)
                + (close ? BonsplitTabBarMetrics.tabMinWidth - 12 : 40),
                y: editorFrame.maxY + BonsplitTabBarMetrics.barHeight / 2)
        }
        func click(_ point: NSPoint) {
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, eventNumber: 1, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
                window.sendEvent(event)
            }
        }
        store.openDocument("src/main.rs")
        pump(until: { editorReady("src/main.rs") }, timeout: 3)
        pump(until: { false }, timeout: 0.15)
        precondition(editorContainer(in: window.contentView) === originalEditor, "Switching tabs replaced the editor")
        click(tabPoint(0))
        pump(until: { store.activeDocument?.id == "README.md" && editorReady("README.md") }, timeout: 3)
        precondition(store.activeDocument?.id == "README.md", "Tab click did not open its document")
        click(tabPoint(1))
        pump(until: { store.activeDocument?.id == "src/main.rs" && editorReady("src/main.rs") }, timeout: 3)
        precondition(store.activeDocument?.id == "src/main.rs", "Second tab click did not open its document")
        let tabText = originalEditor.textView!
        window.makeFirstResponder(tabText)
        tabText.insertText("// edited", replacementRange: NSRange(location: 0, length: 0))
        tabText.breakUndoCoalescing()
        tabText.setSelectedRange(NSRange(location: 0, length: 2))
        precondition(tabText.undoManager?.canUndo == true)
        let identityBeforeRename = store.editorDocumentIdentity
        precondition(store.renameEntry("src/main.rs", newName: "renamed.rs"))
        pump(until: { !store.backendBusy && originalEditor.fileName == "src/renamed.rs" }, timeout: 3)
        precondition(editorContainer(in: window.contentView) === originalEditor, "Native rename replaced the editor")
        precondition(tabText.selectedRange() == NSRange(location: 0, length: 2) && tabText.undoManager?.canUndo == true,
            "Native tabs lost selection or undo history during rename: selection=\(tabText.selectedRange()), undo=\(String(describing: tabText.undoManager?.canUndo)), identity=\(identityBeforeRename)→\(store.editorDocumentIdentity)")
        store.saveDocument()
        store.openDocument("README.md")
        pump(until: { editorReady("README.md") }, timeout: 3)
        pump(until: { false }, timeout: 0.15)
        click(tabPoint(0, close: true))
        pump(until: { !store.openDocuments.contains("README.md") }, timeout: 3)
        precondition(!store.openDocuments.contains("README.md"), "Tab close button did not close its document")
        store.openDocument("README.md")
        pump(until: { editorReady("README.md") }, timeout: 3)
        store.openDocument("notes.md")
        pump(until: { editorReady("notes.md") }, timeout: 3)
        store.openDocument("README.md")
        pump(until: { editorReady("README.md") }, timeout: 3)
        print("macOS document tab selection/close/editor identity/rename smoke test passed")
        if let capture = ProcessInfo.processInfo.environment["NATIVE_WORKSPACE_SMOKE_SCREENSHOT"] {
            pump(until: { false }, timeout: 0.3)
            if CGPreflightScreenCaptureAccess() {
                let task = Process()
                task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                task.arguments = ["-x", "-l", String(window.windowNumber), capture]
                try task.run()
                task.waitUntilExit()
                precondition(task.terminationStatus == 0, "Could not capture the smoke window")
            } else if let content = window.contentView,
                      let bitmap = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
                content.cacheDisplay(in: content.bounds, to: bitmap)
                try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: capture))
            }
        }
        let destinationPoint = destination.convert(NSPoint(x: min(100, destination.bounds.width / 2), y: destination.bounds.midY), to: nil)
        var pendingAt: TimeInterval?
        let subscription = store.$pendingRelocation.sink { relocation in
            if relocation?.target == "src/README.md" { pendingAt = ProcessInfo.processInfo.systemUptime }
        }
        defer { subscription.cancel() }
        queue.suspend()
        var resumed = false
        defer { if !resumed { queue.resume() } }
        func dropView(in view: NSView?) -> SidebarNativeDropTarget.DropView? {
            guard let view else { return nil }
            if let target = view as? SidebarNativeDropTarget.DropView,
                target.visibleRect.contains(target.convert(destinationPoint, from: nil)) { return target }
            for child in view.subviews {
                if let target = dropView(in: child) { return target }
            }
            return nil
        }
        source.configuration?.begin()
        guard let target = dropView(in: window.contentView) else { fatalError("Missing native destination") }
        var previewHidden = false
        state.setPreviewDismissal { previewHidden = true }
        let info = LocalDragInfo(window: window, source: source, point: destinationPoint)
        let releasedAt = ProcessInfo.processInfo.systemUptime
        precondition(target.draggingEntered(info) == .move)
        precondition(target.prepareForDragOperation(info))
        precondition(previewHidden && !info.animatesToDestination, "Preview remained until the import callback")
        precondition(target.performDragOperation(info))
        pump(until: { sourceView("src/README.md", in: window.contentView) != nil }, timeout: 3)
        let visibleAt = ProcessInfo.processInfo.systemUptime
        guard let pendingAt else {
            fatalError("Drop never reached the workspace move handler; error=\(String(describing: store.errorMessage))")
        }
        let callbackMs = (pendingAt - releasedAt) * 1000
        let visibleMs = (visibleAt - releasedAt) * 1000
        print("native drop callback → pending row: \(Int(callbackMs))ms; → visible destination: \(Int(visibleMs))ms")
        precondition(sourceView("src/README.md", in: window.contentView) != nil, "Destination row never became visible")
        precondition(state.source == nil, "Release retained the native drag state")
        precondition(store.pendingRelocation?.target == "src/README.md", "Pending state ended before filesystem confirmation")
        precondition(!FileManager.default.fileExists(atPath: root.appendingPathComponent("src/README.md").path), "Filesystem gate did not hold the operation")
        precondition(callbackMs < 250 && visibleMs < 250, "Native drop stalled before the optimistic row was painted")
        var ticks = 0
        let heartbeat = Timer(timeInterval: 0.01, repeats: true) { _ in ticks += 1 }
        RunLoop.main.add(heartbeat, forMode: .common)
        let waitUntil = ProcessInfo.processInfo.systemUptime + 0.5
        pump(until: { ProcessInfo.processInfo.systemUptime >= waitUntil }, timeout: 1)
        heartbeat.invalidate()
        precondition(ticks >= 20, "UI event loop stalled while filesystem confirmation was blocked")
        queue.resume()
        resumed = true
        pump(until: { !store.backendBusy && store.pendingRelocation == nil }, timeout: 3)
        precondition(FileManager.default.fileExists(atPath: root.appendingPathComponent("src/README.md").path))
        if CommandLine.arguments.contains("--record-drop") {
            let deliveredBy = ProcessInfo.processInfo.systemUptime + 1
            pump(until: { ProcessInfo.processInfo.systemUptime >= deliveredBy }, timeout: 2)
        }
        print("macOS workspace native drop/visible pending row/responsive UI/confirmation smoke test passed")
    }
}
