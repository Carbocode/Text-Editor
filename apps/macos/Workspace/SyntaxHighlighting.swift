import AppKit

enum SyntaxHighlighting {
    static func apply(to view: NSTextView, fileName: String) {
        guard let storage = view.textStorage else { return }
        let text = storage.string
        let full = NSRange(location: 0, length: (text as NSString).length)
        let font = NSFont.monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        storage.beginEditing()
        storage.addAttributes([.font: font, .foregroundColor: NSColor.textColor], range: full)
        for token in highlightDocument(fileName: fileName, text: text) {
            let range = NSRange(location: Int(token.startUtf16), length: Int(token.endUtf16 - token.startUtf16))
            let color: NSColor = switch token.kind {
            case 1: .systemPurple
            case 2: .systemOrange
            case 3: .systemGreen
            case 4: .secondaryLabelColor
            default: .systemBlue
            }
            storage.addAttribute(.foregroundColor, value: color, range: range)
        }
        storage.endEditing()
    }
}
