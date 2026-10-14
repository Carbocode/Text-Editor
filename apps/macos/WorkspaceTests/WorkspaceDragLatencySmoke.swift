import AppKit
import Combine
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
