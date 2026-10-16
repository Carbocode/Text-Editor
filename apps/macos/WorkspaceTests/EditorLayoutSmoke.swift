import AppKit
import SwiftUI

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
        // Short documents must accept clicks across the viewport, including the final empty line.
        let clickScrollView = NoWrapTextEditor.makeScrollView(text: "first\nsecond\n")
        let clickContainer = EditorContainer(scrollView: clickScrollView, fileName: "click.txt")
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 240),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = clickContainer
        clickContainer.layoutSubtreeIfNeeded()
        clickContainer.refreshContent()
        let clickTextView = clickContainer.textView as! EditorTextView
        clickTextView.layoutManager!.ensureLayout(for: clickTextView.textContainer!)
        precondition(clickTextView.frame.width >= clickScrollView.contentSize.width)
        precondition(clickTextView.frame.height >= clickScrollView.contentSize.height)
        func click(_ point: NSPoint, modifiers: NSEvent.ModifierFlags = []) {
            let location = clickTextView.convert(point, to: nil)
            let event = NSEvent.mouseEvent(with: .leftMouseDown, location: location, modifierFlags: modifiers,
                                          timestamp: 0, windowNumber: window.windowNumber, context: nil,
                                          eventNumber: 0, clickCount: 1, pressure: 1)!
            let release = NSEvent.mouseEvent(with: .leftMouseUp, location: location, modifierFlags: [],
                                            timestamp: 0.1, windowNumber: window.windowNumber, context: nil,
                                            eventNumber: 1, clickCount: 1, pressure: 0)!
            NSApplication.shared.postEvent(release, atStart: true)
            let target = window.contentView!.hitTest(window.contentView!.convert(location, from: nil))!
            target.mouseDown(with: event)
        }
        let height = clickContainer.lineHeight
        let right = clickScrollView.contentSize.width - 5
        click(NSPoint(x: right, y: clickTextView.textContainerOrigin.y + height * 2.5))
        precondition(clickTextView.selectedRange() == NSRange(location: 13, length: 0))
        let lastLine = clickTextView.activeLineRect!
        precondition(abs(lastLine.minY - (clickTextView.textContainerOrigin.y + height * 2)) < 1)
        precondition(lastLine.width == clickTextView.bounds.width)
        click(NSPoint(x: right, y: clickTextView.textContainerOrigin.y + height * 0.5))
        precondition(clickTextView.selectedRange() == NSRange(location: 5, length: 0))
        precondition(clickTextView.activeLineRect!.minY < lastLine.minY)
        click(NSPoint(x: -20, y: clickTextView.textContainerOrigin.y + height * 1.5))
        precondition(clickTextView.selectedRange() == NSRange(location: 6, length: 0))
        clickTextView.setSelectedRange(NSRange(location: 13, length: 0))
        precondition(clickTextView.activeLineRect == lastLine)
        clickTextView.string = "first\nsecond"
        clickContainer.refreshContent()
        click(NSPoint(x: right, y: clickTextView.textContainerOrigin.y + height * 1.5))
        precondition(clickTextView.selectedRange() == NSRange(location: 12, length: 0))
        precondition(abs(clickTextView.activeLineRect!.minY - (clickTextView.textContainerOrigin.y + height)) < 1)
        clickTextView.string = ""
        clickContainer.refreshContent()
        click(NSPoint(x: right, y: clickTextView.textContainerOrigin.y + height * 0.5))
        precondition(clickTextView.selectedRange() == NSRange(location: 0, length: 0))
        precondition(clickTextView.activeLineRect!.height > 0)
        // A trailing empty line must also be reachable before keyboard navigation in a long document.
        clickTextView.string = String(repeating: longLine + "\n", count: 80)
        clickTextView.setSelectedRange(NSRange(location: 0, length: 0))
        clickContainer.refreshContent()
        let manager = clickTextView.layoutManager!
        manager.ensureLayout(for: clickTextView.textContainer!)
        let finalRow = manager.extraLineFragmentRect.offsetBy(dx: 0, dy: clickTextView.textContainerOrigin.y)
        let clip = clickScrollView.contentView
        clip.scroll(to: NSPoint(x: 0, y: max(0, clickTextView.bounds.height - clip.bounds.height)))
        clickScrollView.reflectScrolledClipView(clip)
        precondition(finalRow.maxY <= clickTextView.bounds.maxY, "Final empty line is outside the clickable text view")
        for x in [CGFloat(24), clickScrollView.contentSize.width / 2, clickScrollView.contentSize.width - 24] {
            for y in [finalRow.minY + 1, finalRow.midY, finalRow.maxY - 1] {
                clickTextView.setSelectedRange(NSRange(location: 0, length: 0))
                click(NSPoint(x: x, y: y))
                precondition(clickTextView.selectedRange().location == clickTextView.string.utf16.count,
                             "Clicking the last empty row did not move the caret to EOF")
            }
        }
        let end = clickTextView.string.utf16.count
        clickTextView.setSelectedRange(NSRange(location: 0, length: 0))
        click(NSPoint(x: 24, y: finalRow.midY), modifiers: .shift)
        precondition(clickTextView.selectedRange() == NSRange(location: 0, length: end))
        click(NSPoint(x: 24, y: finalRow.midY))
        let dragPoint = clickTextView.convert(NSPoint(x: 24, y: finalRow.minY - height / 2), to: nil)
        let drag = NSEvent.mouseEvent(with: .leftMouseDragged, location: dragPoint, modifierFlags: [],
                                     timestamp: 0.2, windowNumber: window.windowNumber, context: nil,
                                     eventNumber: 2, clickCount: 1, pressure: 1)!
        clickTextView.mouseDragged(with: drag)
        precondition(clickTextView.selectedRange().length > 0)
        precondition(NSMaxRange(clickTextView.selectedRange()) == end)
        for source in ["first\r\nsecond\r\n", "first\n\n\n", "🙂\n"] {
            clickTextView.string = source
            clickTextView.setSelectedRange(NSRange(location: 0, length: 0))
            clickContainer.refreshContent()
            manager.ensureLayout(for: clickTextView.textContainer!)
            clip.scroll(to: .zero)
            clickScrollView.reflectScrolledClipView(clip)
            let emptyY = clickTextView.textContainerOrigin.y + manager.extraLineFragmentRect.midY
            click(NSPoint(x: 24, y: emptyY))
            precondition(clickTextView.selectedRange().location == source.utf16.count)
        }
        // Reuse the real representable while switching tabs, including equal text.
        let host = NSHostingView(rootView: NoWrapTextEditor(documentID: "a.txt", documentIdentity: 1,
            textRevision: 1, text: .constant("first"), searchRange: nil, searchNavigationRevision: 0))
        let tabWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 500, height: 240),
                                 styleMask: [.titled], backing: .buffered, defer: false)
        tabWindow.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        func findEditor(_ view: NSView) -> NSTextView? {
            if let text = view as? NSTextView { return text }
            return view.subviews.lazy.compactMap { findEditor($0) }.first
        }
        let tabEditor = findEditor(host)!
        tabWindow.makeFirstResponder(tabEditor)
        tabEditor.insertText(" edited", replacementRange: NSRange(location: 5, length: 0))
        tabEditor.breakUndoCoalescing()
        precondition(tabEditor.undoManager?.canUndo == true, "Fixture must contain an undoable edit")
        host.rootView = NoWrapTextEditor(documentID: "b.txt", documentIdentity: 2,
            textRevision: 2, text: .constant("first edited"), searchRange: nil, searchNavigationRevision: 0)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        precondition(findEditor(host) === tabEditor, "Tab switching should reuse the native editor")
        precondition(tabEditor.undoManager?.canUndo == false, "Undo from a previous tab must not change this document")
        tabEditor.insertText("!", replacementRange: NSRange(location: tabEditor.string.utf16.count, length: 0))
        tabEditor.breakUndoCoalescing()
        tabEditor.setSelectedRange(NSRange(location: 1, length: 2))
        host.rootView = NoWrapTextEditor(documentID: "renamed-b.txt", documentIdentity: 2,
            textRevision: 3, text: .constant(tabEditor.string), searchRange: nil, searchNavigationRevision: 0)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        precondition(tabEditor.undoManager?.canUndo == true, "A rename must preserve the document's undo history")
        precondition(tabEditor.selectedRange() == NSRange(location: 1, length: 2), "A rename must preserve selection")
        host.rootView = NoWrapTextEditor(documentID: "c.txt", documentIdentity: 3,
            textRevision: 4, text: .constant("third document"), searchRange: nil, searchNavigationRevision: 0)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        precondition(tabEditor.string == "third document")
        precondition(tabEditor.undoManager?.canUndo == false, "Different tab contents must also isolate undo")
        print("AppKit tab undo isolation/rename selection smoke test passed")
        print("AppKit horizontal overflow smoke test passed")
    }
}
