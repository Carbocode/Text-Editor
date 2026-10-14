import AppKit
import SwiftUI

/// Own the AppKit session so release can remove its image before the tree changes.
struct SidebarNativeDragSource: NSViewRepresentable {
    let path: String
    let directory: Bool
    let type: NSPasteboard.PasteboardType
    let enabled: Bool
    let state: SidebarDragState
    let begin: () -> Void
    let end: () -> Void
    var rowReady: () -> Void = {}

    func makeNSView(context: Context) -> DragView { DragView() }
    func updateNSView(_ view: DragView, context: Context) { view.configuration = self; view.reportReady() }

    final class DragView: NSView, NSDraggingSource {
        var configuration: SidebarNativeDragSource?
        private var monitor: Any?
        private var pressedAt: NSPoint?
        private var session: NSDraggingSession?

        func reportReady() {
            if window != nil && !visibleRect.isEmpty { configuration?.rowReady() }
        }
        override func layout() { super.layout(); reportReady() }
        override func setFrameSize(_ newSize: NSSize) { super.setFrameSize(newSize); reportReady() }

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]) { [weak self] event in
                guard let self else { return event }
                return self.handle(event)
            }
        }

        private func handle(_ event: NSEvent) -> NSEvent? {
            guard event.window === window, let configuration else { return event }
            switch event.type {
            case .leftMouseDown:
                let point = convert(event.locationInWindow, from: nil)
                pressedAt = configuration.enabled && visibleRect.contains(point) ? point : nil
            case .leftMouseUp:
                pressedAt = nil
                if session != nil { configuration.state.hidePreview() }
            case .leftMouseDragged:
                guard session == nil, let origin = pressedAt else { return event }
                let point = convert(event.locationInWindow, from: nil)
                guard hypot(point.x - origin.x, point.y - origin.y) >= 5 else { return event }
                pressedAt = nil
                configuration.begin()
                let payload = NSPasteboardItem()
                payload.setData(Data(configuration.path.utf8), forType: configuration.type)
                let item = NSDraggingItem(pasteboardWriter: payload)
                let image = preview(configuration)
                item.setDraggingFrame(NSRect(origin: point, size: image.size), contents: image)
                let session = beginDraggingSession(with: [item], event: event, source: self)
                session.animatesToStartingPositionsOnCancelOrFail = false
                self.session = session
                configuration.state.setPreviewDismissal { [weak session] in
                    session?.enumerateDraggingItems(options: [], for: nil,
                        classes: [NSPasteboardItem.self], searchOptions: [:]) { item, _, _ in
                        item.setDraggingFrame(item.draggingFrame, contents: nil)
                    }
                }
                return nil
            default: break
            }
            return event
        }

        private func preview(_ configuration: SidebarNativeDragSource) -> NSImage {
            let title = configuration.path.split(separator: "/").last.map(String.init) ?? configuration.path
            let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor]
            let width = min(300, (title as NSString).size(withAttributes: attributes).width + 42)
            let image = NSImage(size: NSSize(width: width, height: 32))
            image.lockFocus()
            NSColor.controlBackgroundColor.setFill()
            NSBezierPath(roundedRect: NSRect(origin: .zero, size: image.size), xRadius: 6, yRadius: 6).fill()
            NSImage(systemSymbolName: configuration.directory ? "folder.fill" : "doc.fill", accessibilityDescription: nil)?
                .draw(in: NSRect(x: 10, y: 8, width: 16, height: 16))
            (title as NSString).draw(in: NSRect(x: 32, y: 8, width: width - 38, height: 18), withAttributes: attributes)
            image.unlockFocus()
            return image
        }

        func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
            context == .withinApplication ? .move : []
        }
        func ignoreModifierKeys(for session: NSDraggingSession) -> Bool { true }
        func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
            configuration?.state.finish()
            configuration?.end()
            self.session = nil
        }
        deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
    }
}
