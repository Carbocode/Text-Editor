import AppKit
import SwiftUI

struct NoWrapTextEditor: NSViewRepresentable {
    let documentID: String
    @Binding var text: String
    let searchRange: NSRange?
    let searchNavigationRevision: UInt64

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> EditorContainer {
        let scrollView = Self.makeScrollView(text: text)
        let container = EditorContainer(scrollView: scrollView)
        context.coordinator.container = container
        (scrollView.documentView as? NSTextView)?.delegate = context.coordinator
        if let textView = scrollView.documentView as? NSTextView {
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
        }
        return container
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

    func updateNSView(_ container: EditorContainer, context: Context) {
        let scrollView = container.scrollView
        guard let textView = scrollView.documentView as? NSTextView else { return }
        context.coordinator.parent = self
        let switchedDocument = context.coordinator.documentID != documentID
        if textView.string != text {
            textView.string = text
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
        }
        if switchedDocument {
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
            textView.setSelectedRange(NSRange(location: 0, length: 0))
            scrollView.contentView.scroll(to: .zero)
            scrollView.reflectScrolledClipView(scrollView.contentView)
            context.coordinator.documentID = documentID
        }
        if context.coordinator.searchNavigationRevision != searchNavigationRevision {
            context.coordinator.searchNavigationRevision = searchNavigationRevision
            if let range = searchRange,
               range.location + range.length <= textView.string.utf16.count {
                Self.selectSearchRange(range, in: textView)
            } else if searchRange == nil {
                textView.setSelectedRange(NSRange(location: textView.selectedRange().location, length: 0))
            }
        }
        container.refreshChrome()
    }

    static func selectSearchRange(_ range: NSRange, in textView: NSTextView) {
        textView.setSelectedRange(range)
        textView.scrollRangeToVisible(range)
        textView.window?.makeFirstResponder(textView)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: NoWrapTextEditor
        var documentID: String
        var searchNavigationRevision: UInt64 = 0
        weak var container: EditorContainer?

        init(_ parent: NoWrapTextEditor) {
            self.parent = parent
            documentID = parent.documentID
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
            parent.text = textView.string
            container?.refreshChrome()
        }
    }
}

final class EditorContainer: NSView {
    let scrollView: NSScrollView
    private let gutter = EditorGutter()
    private let minimap = EditorMinimap()
    private var scrollObserver: NSObjectProtocol?

    init(scrollView: NSScrollView) {
        self.scrollView = scrollView
        super.init(frame: .zero)
        addSubview(gutter)
        addSubview(scrollView)
        addSubview(minimap)
        gutter.container = self
        minimap.container = self
        scrollView.contentView.postsBoundsChangedNotifications = true
        scrollObserver = NotificationCenter.default.addObserver(
            forName: NSView.boundsDidChangeNotification,
            object: scrollView.contentView,
            queue: .main
        ) { [weak self] _ in self?.refreshChrome() }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

    deinit {
        if let scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }
    }

    override func layout() {
        super.layout()
        gutter.frame = NSRect(x: 0, y: 0, width: 48, height: bounds.height)
        scrollView.frame = NSRect(x: 48, y: 0, width: max(0, bounds.width - 128), height: bounds.height)
        minimap.frame = NSRect(x: max(48, bounds.width - 80), y: 0, width: 80, height: bounds.height)
        refreshChrome()
    }

    func refreshChrome() {
        gutter.needsDisplay = true
        minimap.needsDisplay = true
    }

    var textView: NSTextView? { scrollView.documentView as? NSTextView }
    var lineHeight: CGFloat {
        guard let textView, let font = textView.font else { return 16 }
        return textView.layoutManager?.defaultLineHeight(for: font) ?? 16
    }
}

private final class EditorGutter: NSView {
    weak var container: EditorContainer?
    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard let container, let textView = container.textView else { return }
        let lineHeight = container.lineHeight
        let scrollY = container.scrollView.contentView.bounds.origin.y
        let count = textView.string.reduce(1) { $1 == "\n" ? $0 + 1 : $0 }
        let first = min(count, max(0, Int((scrollY - textView.textContainerInset.height) / lineHeight)))
        let last = min(count, first + Int(ceil(bounds.height / lineHeight)) + 3)
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor
        ]
        for line in first..<last {
            let label = String(line + 1) as NSString
            let size = label.size(withAttributes: attributes)
            let y = textView.textContainerInset.height + CGFloat(line) * lineHeight - scrollY
            label.draw(at: NSPoint(x: bounds.width - size.width - 8, y: y), withAttributes: attributes)
        }
    }
}

private final class EditorMinimap: NSView {
    weak var container: EditorContainer?
    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard let container, let textView = container.textView else { return }
        let lines = textView.string.split(separator: "\n", omittingEmptySubsequences: false)
        let lineStride = max(1, Int(ceil(CGFloat(lines.count) / max(1, bounds.height / 2))))
        NSColor.secondaryLabelColor.withAlphaComponent(0.4).setFill()
        for index in stride(from: 0, to: lines.count, by: lineStride) {
            let line = lines[index]
            NSRect(x: 4, y: CGFloat(index) / CGFloat(lines.count) * bounds.height,
                   width: min(bounds.width - 8, CGFloat(line.count) * 1.2), height: 1).fill()
        }
        let clip = container.scrollView.contentView
        let total = max(1, textView.bounds.height)
        NSColor.secondaryLabelColor.withAlphaComponent(0.2).setFill()
        NSRect(x: 0, y: clip.bounds.origin.y / total * bounds.height,
               width: bounds.width, height: max(8, clip.bounds.height / total * bounds.height)).fill()
    }

    override func mouseDown(with event: NSEvent) {
        guard let container, let textView = container.textView else { return }
        let y = convert(event.locationInWindow, from: nil).y
        let clip = container.scrollView.contentView
        let limit = max(0, textView.bounds.height - clip.bounds.height)
        let target = min(limit, max(0, y / max(1, bounds.height) * textView.bounds.height - clip.bounds.height / 2))
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: target))
        container.scrollView.reflectScrolledClipView(clip)
    }
}
