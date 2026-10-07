import AppKit
import SwiftUI

struct NoWrapTextEditor: NSViewRepresentable {
    let documentID: String
    @Binding var text: String

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = Self.makeScrollView(text: text)
        (scrollView.documentView as? NSTextView)?.delegate = context.coordinator
        return scrollView
    }

    static func makeScrollView(text: String) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.borderType = .noBorder
        scrollView.hasHorizontalScroller = true
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true

        let textView = NSTextView(frame: NSRect(origin: .zero, size: scrollView.contentSize))
        textView.isRichText = false
        textView.allowsUndo = true
        textView.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        textView.textContainerInset = NSSize(width: 12, height: 12)
        textView.minSize = NSSize(width: 0, height: scrollView.contentSize.height)
        let unlimited = CGFloat(Float.greatestFiniteMagnitude)
        textView.maxSize = NSSize(width: unlimited, height: unlimited)
        textView.isHorizontallyResizable = true
        textView.isVerticallyResizable = true
        textView.autoresizingMask = [.width, .height]
        textView.textContainer?.containerSize = NSSize(width: unlimited, height: unlimited)
        textView.textContainer?.widthTracksTextView = false
        textView.string = text
        scrollView.documentView = textView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? NSTextView else { return }
        context.coordinator.parent = self
        let switchedDocument = context.coordinator.documentID != documentID
        if textView.string != text {
            textView.string = text
        }
        if switchedDocument {
            textView.setSelectedRange(NSRange(location: 0, length: 0))
            scrollView.contentView.scroll(to: .zero)
            scrollView.reflectScrolledClipView(scrollView.contentView)
            context.coordinator.documentID = documentID
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: NoWrapTextEditor
        var documentID: String

        init(_ parent: NoWrapTextEditor) {
            self.parent = parent
            documentID = parent.documentID
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            parent.text = textView.string
        }
    }
}
