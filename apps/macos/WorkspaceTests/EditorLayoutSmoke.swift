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
        print("AppKit horizontal overflow smoke test passed")
    }
}
