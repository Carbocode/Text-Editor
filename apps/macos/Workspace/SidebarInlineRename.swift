import AppKit
import SwiftUI

/// AppKit's field editor preserves the native selection, Return, and Escape behavior.
struct SidebarInlineRename: NSViewRepresentable {
    @Binding var name: String
    let directory: Bool
    let submit: () -> Void
    let cancel: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTextField {
        let field = RenameTextField(string: name)
        field.font = .systemFont(ofSize: NSFont.systemFontSize)
        field.focusRingType = .exterior
        field.delegate = context.coordinator
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setAccessibilityLabel("Rename")
        let full = name as NSString
        let dot = directory ? NSNotFound : full.range(of: ".", options: .backwards).location
        field.selectLength = dot != NSNotFound && dot > 0 ? dot : full.length
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != name { field.stringValue = name }
    }

    final class RenameTextField: NSTextField {
        var selectLength = 0
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard window != nil else { return }
            DispatchQueue.main.async { [weak self] in
                guard let self, let window = self.window else { return }
                window.makeFirstResponder(self)
                self.currentEditor()?.selectedRange = NSRange(location: 0, length: self.selectLength)
            }
        }
    }

    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: SidebarInlineRename
        init(_ parent: SidebarInlineRename) { self.parent = parent }

        func controlTextDidChange(_ notification: Notification) {
            if let field = notification.object as? NSTextField { parent.name = field.stringValue }
        }

        func controlTextDidEndEditing(_ notification: Notification) { parent.cancel() }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            if selector == #selector(NSResponder.insertNewline(_:)) { parent.submit(); return true }
            if selector == #selector(NSResponder.cancelOperation(_:)) { parent.cancel(); return true }
            return false
        }
    }
}
