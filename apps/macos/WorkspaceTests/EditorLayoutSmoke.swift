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
        let containerView = EditorContainer(scrollView: scrollView)
        containerView.frame = NSRect(x: 0, y: 0, width: 500, height: 240)
        containerView.layoutSubtreeIfNeeded()
        precondition(containerView.scrollView.frame.minX == 48)
        precondition(containerView.scrollView.frame.width == 372)
        let codeView = NoWrapTextEditor.makeScrollView(text: "let value = 42")
        let codeTextView = codeView.documentView as! NSTextView
        SyntaxHighlighting.apply(to: codeTextView, fileName: "main.rs")
        precondition(codeTextView.string == "let value = 42")
        let keywordColor = codeTextView.textStorage?.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor
        precondition(keywordColor == .systemPurple)
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
