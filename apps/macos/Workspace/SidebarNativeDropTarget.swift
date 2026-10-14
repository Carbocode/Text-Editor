import AppKit
import SwiftUI

/// Accept internal drops before SwiftUI item-provider decoding or native landing.
struct SidebarNativeDropTarget: NSViewRepresentable {
    let type: NSPasteboard.PasteboardType
    let state: SidebarDragState
    let canDrop: () -> Bool
    let entered: () -> Void
    let exited: () -> Void
    let dropped: () -> Bool
    var beginDrop: () -> Void = {}
    var previewHidden: () -> Void = {}

    func makeNSView(context: Context) -> DropView { DropView() }
    func updateNSView(_ view: DropView, context: Context) {
        view.configuration = self
        view.registerForDraggedTypes([type])
    }

    final class DropView: NSView {
        var configuration: SidebarNativeDropTarget?
        override func hitTest(_ point: NSPoint) -> NSView? {
            guard configuration?.state.source != nil, !isHidden,
                  visibleRect.contains(convert(point, from: superview)) else { return nil }
            return self
        }
        private func owns(_ sender: any NSDraggingInfo) -> Bool {
            guard let source = sender.draggingSource as? SidebarNativeDragSource.DragView,
                  let state = source.configuration?.state,
                  let configuration else { return false }
            return state === configuration.state && state.source != nil
        }
        override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
            guard owns(sender), let configuration else { return [] }
            sender.animatesToDestination = false
            configuration.entered()
            return configuration.canDrop() ? .move : []
        }
        override func draggingUpdated(_ sender: any NSDraggingInfo) -> NSDragOperation {
            draggingEntered(sender)
        }
        override func draggingExited(_ sender: (any NSDraggingInfo)?) {
            configuration?.exited()
        }
        override func prepareForDragOperation(_ sender: any NSDraggingInfo) -> Bool {
            sender.animatesToDestination = false
            guard owns(sender), let configuration, configuration.canDrop() else { return false }
            configuration.beginDrop()
            // This hook runs before AppKit concludes the drag and invokes perform.
            configuration.state.hidePreview()
            configuration.previewHidden()
            return true
        }
        override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
            guard owns(sender), let configuration else { return false }
            return configuration.dropped()
        }
        override func wantsPeriodicDraggingUpdates() -> Bool { false }
    }
}
