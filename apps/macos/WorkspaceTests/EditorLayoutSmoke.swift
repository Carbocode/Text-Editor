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
