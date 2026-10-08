import AppKit

enum SyntaxHighlighting {
    // AppKit lays out the viewport; styling only its lines keeps editing responsive in long files.
    static func apply(to view: NSTextView, fileName: String) {
        guard let storage = view.textStorage, storage.length > 0,
              let manager = view.layoutManager, let container = view.textContainer else { return }
        let full = NSRange(location: 0, length: storage.length)
        let range: NSRange
        if storage.length < 20_000 {
            range = full
        } else {
            let glyphs = manager.glyphRange(forBoundingRect: view.visibleRect, in: container)
            let characters = manager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
            let lines = (storage.string as NSString).lineRange(for: characters)
            if lines.length > 16_000 {
                let cursor = view.selectedRange().location
                let anchor = min(NSMaxRange(lines), max(lines.location, cursor))
                let start = max(lines.location, anchor - 8_000)
                range = NSRange(location: start, length: min(16_000, NSMaxRange(lines) - start))
            } else {
                range = lines
            }
        }
        guard range.length > 0 else { return }
        let content = (storage.string as NSString).substring(with: range)
        let tokens = highlightDocument(fileName: fileName, text: content)
        storage.beginEditing()
        storage.addAttribute(.foregroundColor, value: NSColor.textColor, range: range)
        for token in tokens {
            let colored = NSRange(
                location: range.location + Int(token.startUtf16),
                length: Int(token.endUtf16 - token.startUtf16)
            )
            let color: NSColor = switch token.kind {
            case 1: .systemPurple
            case 2: .systemOrange
            case 3: .systemGreen
            case 4: .secondaryLabelColor
            default: .systemBlue
            }
            storage.addAttribute(.foregroundColor, value: color, range: colored)
        }
        storage.endEditing()
    }
}
