import AppKit

@main
struct EditorLayoutSmoke {
    static func main() {
        _ = NSApplication.shared
        let longLine = String(repeating: "M", count: 500)
        let scrollView = NoWrapTextEditor.makeScrollView(text: longLine)
        scrollView.frame = NSRect(x: 0, y: 0, width: 320, height: 240)
        scrollView.layoutSubtreeIfNeeded()
        guard let textView = scrollView.documentView as? NSTextView,
              let container = textView.textContainer,
              let layoutManager = textView.layoutManager else {
            fatalError("No NSTextView in the scroll view")
        }
        layoutManager.ensureLayout(for: container)
        precondition(scrollView.hasHorizontalScroller)
        precondition(!container.widthTracksTextView)
        precondition(layoutManager.usedRect(for: container).width > scrollView.contentSize.width)
        precondition(textView.frame.width > scrollView.contentSize.width)
        let containerView = EditorContainer(scrollView: scrollView, fileName: "long.txt")
        containerView.frame = NSRect(x: 0, y: 0, width: 500, height: 240)
        containerView.layoutSubtreeIfNeeded()
        precondition(containerView.scrollView.frame.minX == 48)
        precondition(containerView.scrollView.frame.width == 352)
        containerView.refreshContent()
        containerView.noteReplacement(in: textView, range: NSRange(location: 0, length: 0), replacement: "\n")
        textView.string = "\n" + longLine
        containerView.refreshContent(incremental: true)
        precondition(containerView.displayedLineCount == 2)
        let codeView = NoWrapTextEditor.makeScrollView(text: "let value = 42")
        let codeTextView = codeView.documentView as! NSTextView
        SyntaxHighlighting.apply(to: codeTextView, fileName: "main.rs")
        precondition(codeTextView.string == "let value = 42")
        let keywordColor = codeTextView.textStorage?.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
        precondition(keywordColor == .systemPurple)
        let shortMap = EditorMinimapRaster.build(text: "  let x = 1\n", fileName: "main.rs")
        precondition(shortMap.contentHeight == 4)
        let shortBitmap = shortMap.image(width: 100, height: 240, offset: 0)!.representations[0] as! NSBitmapImageRep
        precondition(shortBitmap.colorAt(x: 4, y: 0)!.alphaComponent == 0)
        precondition(shortBitmap.colorAt(x: 6, y: 0)!.alphaComponent > 0)
        let alternating = EditorMinimapRaster.build(
            text: String(repeating: "MMMMMMMMMMMMMMMM\n\n", count: 400), fileName: "bun.lock"
        )
        precondition(alternating.contentHeight > 240)
        let middle = alternating.image(width: 100, height: 20, offset: 400)!.representations[0] as! NSBitmapImageRep
        precondition(middle.colorAt(x: 4, y: 0)!.alphaComponent > 0)
        precondition(middle.colorAt(x: 4, y: 2)!.alphaComponent == 0)
        precondition(middle.colorAt(x: 4, y: 4)!.alphaComponent > 0)
        let searchable = String(repeating: "filler\n", count: 80) + "needle"
        let searchView = NoWrapTextEditor.makeScrollView(text: searchable)
        searchView.frame = NSRect(x: 0, y: 0, width: 320, height: 240)
        searchView.layoutSubtreeIfNeeded()
        let searchTextView = searchView.documentView as! NSTextView
        let match = (searchable as NSString).range(of: "needle")
        NoWrapTextEditor.selectSearchRange(match, in: searchTextView)
        precondition(searchTextView.selectedRange() == match)
        precondition(searchView.contentView.bounds.origin.y > 0)
        print("AppKit horizontal overflow smoke test passed")
    }
}
