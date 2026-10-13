import AppKit
import SwiftUI

struct NoWrapTextEditor: NSViewRepresentable {
    let documentID: String
    let textRevision: UInt64
    @Binding var text: String
    let searchRange: NSRange?
    let searchNavigationRevision: UInt64

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> EditorContainer {
        let scrollView = Self.makeScrollView(text: text)
        let container = EditorContainer(scrollView: scrollView, fileName: documentID)
        context.coordinator.container = container
        (scrollView.documentView as? NSTextView)?.delegate = context.coordinator
        if let textView = scrollView.documentView as? NSTextView {
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
        }
        container.refreshContent()
        return container
    }

    static func makeScrollView(text: String) -> NSScrollView {
        let scrollView = NSScrollView()
        scrollView.borderType = .noBorder
        scrollView.hasHorizontalScroller = true
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true

        let textView = EditorTextView(frame: NSRect(origin: .zero, size: scrollView.contentSize))
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
        if switchedDocument || context.coordinator.textRevision != textRevision {
            context.coordinator.textRevision = textRevision
            if switchedDocument {
                container.fileName = documentID
            }
            let changedText = textView.string != text
            if changedText {
                textView.string = text
            }
            if switchedDocument {
                textView.setSelectedRange(NSRange(location: 0, length: 0))
                scrollView.contentView.scroll(to: .zero)
                scrollView.reflectScrolledClipView(scrollView.contentView)
                context.coordinator.documentID = documentID
            }
            if changedText || switchedDocument {
                SyntaxHighlighting.apply(to: textView, fileName: documentID)
                container.refreshContent()
            }
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
        if switchedDocument { container.refreshChrome() }
    }

    static func selectSearchRange(_ range: NSRange, in textView: NSTextView) {
        textView.setSelectedRange(range)
        textView.scrollRangeToVisible(range)
        textView.window?.makeFirstResponder(textView)
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: NoWrapTextEditor
        var documentID: String
        var textRevision: UInt64
        var searchNavigationRevision: UInt64 = 0
        weak var container: EditorContainer?

        init(_ parent: NoWrapTextEditor) {
            self.parent = parent
            documentID = parent.documentID
            textRevision = parent.textRevision
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            SyntaxHighlighting.apply(to: textView, fileName: documentID)
            parent.text = textView.string
            container?.refreshContent(incremental: true)
        }

        func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange,
                      replacementString: String?) -> Bool {
            container?.noteReplacement(in: textView, range: affectedCharRange,
                                       replacement: replacementString ?? "")
            return true
        }
    }
}

final class EditorTextView: NSTextView {
    static var activeLineColor: NSColor { .selectedContentBackgroundColor.withAlphaComponent(0.10) }
    private var emptyLineSelectionAnchor: Int?

    override func mouseDown(with event: NSEvent) {
        emptyLineSelectionAnchor = nil
        let point = convert(event.locationInWindow, from: nil)
        if event.clickCount == 1,
           string.isEmpty || string.hasSuffix("\n") || string.hasSuffix("\r"),
           let manager = layoutManager, let container = textContainer {
            manager.ensureLayout(forBoundingRect: NSRect(x: 0,
                y: max(0, point.y - textContainerOrigin.y), width: 1, height: 1), in: container)
            let line = manager.extraLineFragmentRect.offsetBy(dx: textContainerOrigin.x,
                                                            dy: textContainerOrigin.y)
            if manager.extraLineFragmentTextContainer === container,
               point.y >= line.minY, point.y < line.maxY {
                // Make the final empty fragment a cursor target across the full text view width.
                let end = (string as NSString).length
                let anchor = event.modifierFlags.contains(.shift) ? selectedRange().location : end
                emptyLineSelectionAnchor = anchor
                window?.makeFirstResponder(self)
                setSelectedRange(NSRange(location: min(anchor, end), length: abs(end - anchor)))
                return
            }
        }
        super.mouseDown(with: event)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let anchor = emptyLineSelectionAnchor else {
            super.mouseDragged(with: event)
            return
        }
        autoscroll(with: event)
        let location = characterIndexForInsertion(at: convert(event.locationInWindow, from: nil))
        setSelectedRange(NSRange(location: min(anchor, location), length: abs(location - anchor)))
    }

    override func mouseUp(with event: NSEvent) {
        if emptyLineSelectionAnchor != nil {
            emptyLineSelectionAnchor = nil
            return
        }
        super.mouseUp(with: event)
    }

    var activeLineRect: NSRect? {
        guard let manager = layoutManager, let container = textContainer else { return nil }
        let location = min(selectedRange().location, (string as NSString).length)
        manager.ensureLayout(forCharacterRange: NSRange(location: location, length: 0))
        let fragment: NSRect
        if location == (string as NSString).length,
           manager.extraLineFragmentTextContainer === container {
            fragment = manager.extraLineFragmentRect
        } else if manager.numberOfGlyphs > 0 {
            let glyph = manager.glyphIndexForCharacter(at: min(location, (string as NSString).length - 1))
            fragment = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
        } else {
            fragment = NSRect(x: 0, y: 0, width: 0,
                              height: manager.defaultLineHeight(for: font ?? .systemFont(ofSize: NSFont.systemFontSize)))
        }
        return NSRect(x: bounds.minX, y: textContainerOrigin.y + fragment.minY,
                      width: bounds.width, height: fragment.height)
    }

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        guard let line = activeLineRect else { return }
        Self.activeLineColor.setFill()
        line.intersection(rect).fill()
    }

    override func setFrameSize(_ newSize: NSSize) {
        let viewport = enclosingScrollView?.contentSize ?? .zero
        super.setFrameSize(NSSize(width: max(newSize.width, viewport.width),
                                  height: max(newSize.height, viewport.height)))
    }
}

final class EditorContainer: NSView {
    let scrollView: NSScrollView
    var fileName: String
    private let gutter = EditorGutter()
    private let minimap = EditorMinimap()
    private var scrollObserver: NSObjectProtocol?
    private var selectionObserver: NSObjectProtocol?
    private var highlightWork: DispatchWorkItem?
    private var minimapWork: DispatchWorkItem?
    private var minimapGeneration = 0
    private var incrementalLineCountReady = false

    init(scrollView: NSScrollView, fileName: String) {
        self.scrollView = scrollView
        self.fileName = fileName
        super.init(frame: .zero)
        gutter.wantsLayer = true
        gutter.layer?.masksToBounds = true
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
        ) { [weak self] _ in
            guard let self else { return }
            self.scheduleHighlight()
            self.refreshChrome()
        }
        selectionObserver = NotificationCenter.default.addObserver(
            forName: NSTextView.didChangeSelectionNotification,
            object: scrollView.documentView,
            queue: .main
        ) { [weak self] _ in
            self?.refreshChrome()
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

    deinit {
        if let scrollObserver { NotificationCenter.default.removeObserver(scrollObserver) }
        if let selectionObserver { NotificationCenter.default.removeObserver(selectionObserver) }
        minimapWork?.cancel()
        highlightWork?.cancel()
    }

    override func layout() {
        super.layout()
        gutter.frame = NSRect(x: 0, y: 0, width: 48, height: bounds.height)
        scrollView.frame = NSRect(x: 48, y: 0, width: max(0, bounds.width - 148), height: bounds.height)
        minimap.frame = NSRect(x: max(48, bounds.width - 100), y: 0, width: 100, height: bounds.height)
        if let textView {
            textView.minSize = scrollView.contentSize
            textView.setFrameSize(textView.frame.size)
        }
        refreshChrome()
    }

    func noteReplacement(in view: NSTextView, range: NSRange, replacement: String) {
        let old = view.string as NSString
        guard NSMaxRange(range) <= old.length else { return }
        let removed = old.substring(with: range).utf8.reduce(0) { $0 + ($1 == 10 ? 1 : 0) }
        let inserted = replacement.utf8.reduce(0) { $0 + ($1 == 10 ? 1 : 0) }
        gutter.lineCount = max(1, gutter.lineCount + inserted - removed)
        incrementalLineCountReady = true
    }

    func refreshContent(incremental: Bool = false) {
        if !incremental || !incrementalLineCountReady {
            gutter.lineCount = textView?.string.utf8.reduce(1) { $0 + ($1 == 10 ? 1 : 0) } ?? 1
        }
        incrementalLineCountReady = false
        scheduleMinimap()
        refreshChrome()
    }

    private func scheduleHighlight() {
        highlightWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, let textView = self.textView else { return }
            SyntaxHighlighting.apply(to: textView, fileName: self.fileName)
        }
        highlightWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.04, execute: work)
    }

    private func scheduleMinimap() {
        minimapWork?.cancel()
        minimapGeneration += 1
        let generation = minimapGeneration
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.minimapGeneration == generation, let textView = self.textView else { return }
            let content = textView.string
            let name = self.fileName
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let raster = EditorMinimapRaster.build(text: content, fileName: name)
                DispatchQueue.main.async { [weak self] in
                    guard let self, self.minimapGeneration == generation else { return }
                    self.minimap.setRaster(raster)
                }
            }
        }
        minimapWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.22, execute: work)
    }

    func refreshChrome() {
        textView?.needsDisplay = true
        gutter.needsDisplay = true
        minimap.needsDisplay = true
    }

    var textView: NSTextView? { scrollView.documentView as? NSTextView }
    var displayedLineCount: Int { gutter.lineCount }
    var lineHeight: CGFloat {
        guard let textView, let font = textView.font else { return 16 }
        return textView.layoutManager?.defaultLineHeight(for: font) ?? 16
    }
}

private final class EditorGutter: NSView {
    weak var container: EditorContainer?
    var lineCount = 1
    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard let container, let textView = container.textView else { return }
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: bounds).addClip()
        defer { NSGraphicsContext.restoreGraphicsState() }
        if let line = (textView as? EditorTextView)?.activeLineRect {
            let converted = convert(line, from: textView)
            EditorTextView.activeLineColor.setFill()
            NSRect(x: 0, y: converted.minY, width: bounds.width, height: converted.height).fill()
        }
        let lineHeight = container.lineHeight
        let scrollY = container.scrollView.contentView.bounds.origin.y
        let count = lineCount
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
            guard y + size.height > 0 && y < bounds.height else { continue }
            label.draw(at: NSPoint(x: bounds.width - size.width - 8, y: y), withAttributes: attributes)
        }
    }

    override func mouseDown(with event: NSEvent) {
        guard let textView = container?.textView else { return }
        let point = textView.convert(event.locationInWindow, from: nil)
        let location = textView.characterIndexForInsertion(at: NSPoint(x: textView.textContainerOrigin.x, y: point.y))
        textView.window?.makeFirstResponder(textView)
        textView.setSelectedRange(NSRange(location: location, length: 0))
    }

    override func scrollWheel(with event: NSEvent) {
        container?.scrollView.scrollWheel(with: event)
    }
}

private final class EditorMinimap: NSView {
    weak var container: EditorContainer?
    private var raster: EditorMinimapRaster?
    private var rasterImage: NSImage?
    private var rasterOffset = -1
    private var rasterSize: NSSize = .zero
    private var dragOffset: CGFloat?
    override var isFlipped: Bool { true }

    func setRaster(_ value: EditorMinimapRaster) {
        raster = value
        rasterOffset = -1
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let container, let raster else { return }
        let clip = container.scrollView.contentView
        let visibleHeight = min(CGFloat(raster.contentHeight), clip.bounds.height / container.lineHeight * 2)
        let top = min(max(0, CGFloat(raster.contentHeight) - visibleHeight),
                      max(0, clip.bounds.origin.y / container.lineHeight * 2))
        let maxOffset = max(0, CGFloat(raster.contentHeight) - bounds.height)
        let offset = Int(min(maxOffset, max(0, top - (bounds.height - visibleHeight) / 2)))
        if offset != rasterOffset || rasterSize != bounds.size {
            rasterImage = raster.image(width: Int(bounds.width), height: Int(bounds.height), offset: offset)
            rasterOffset = offset
            rasterSize = bounds.size
        }
        rasterImage?.draw(in: bounds, from: .zero, operation: .sourceOver,
                          fraction: 1, respectFlipped: true, hints: nil)
        NSColor.secondaryLabelColor.withAlphaComponent(0.2).setFill()
        NSRect(x: 0, y: max(0, top - CGFloat(offset)), width: bounds.width,
               height: max(8, min(bounds.height, visibleHeight))).fill()
    }

    override func mouseDown(with event: NSEvent) {
        dragOffset = CGFloat(max(0, rasterOffset))
        scrollTo(event)
    }

    override func mouseDragged(with event: NSEvent) {
        scrollTo(event)
    }

    override func mouseUp(with event: NSEvent) {
        dragOffset = nil
    }

    override func scrollWheel(with event: NSEvent) {
        container?.scrollView.scrollWheel(with: event)
    }

    private func scrollTo(_ event: NSEvent) {
        guard let container, let textView = container.textView, raster != nil else { return }
        let y = convert(event.locationInWindow, from: nil).y
        let clip = container.scrollView.contentView
        let limit = max(0, textView.bounds.height - clip.bounds.height)
        let targetLine = (y + (dragOffset ?? CGFloat(max(0, rasterOffset)))) / 2
        let target = min(limit, max(0, targetLine * container.lineHeight - clip.bounds.height / 2))
        clip.scroll(to: NSPoint(x: clip.bounds.origin.x, y: target))
        container.scrollView.reflectScrolledClipView(clip)
    }
}
