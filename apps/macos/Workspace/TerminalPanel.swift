import AppKit
import SwiftUI

struct TerminalPanelState: Decodable {
    struct Tab: Decodable, Identifiable { var id: UInt64; var title: String; var running: Bool; var busy: Bool; var status: String }
    var revision: UInt64 = 0
    var tabs: [Tab] = []
    var active: UInt64?
    var frame: TerminalGrid?
}
struct TerminalGrid: Decodable {
    struct Cell: Decodable {
        var width: Int; var text: String; var foreground: UInt32; var background: UInt32
        var bold: Bool; var italic: Bool; var underline: Bool
    }
    var cols: Int; var rows: Int; var cells: [Cell]; var cursor: [Int]?; var background: UInt32
}

struct TerminalPanel: View {
    @ObservedObject var store: WorkspaceStore
    @ObservedObject private var feed: TerminalFeed
    init(store: WorkspaceStore) { self.store = store; feed = store.terminalFeed }
    @State private var closing: UInt64?
    var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                if let id = feed.state.active {
                    TerminalSurface(store: store, id: id, frame: feed.state.frame)
                    if let tab = feed.state.tabs.first(where: { $0.id == id }), !tab.status.isEmpty {
                        Text(tab.status).font(.caption).padding(4)
                    }
                } else {
                    Button("New Terminal") { store.terminalAction("new") }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            Divider()
            VStack(spacing: 0) {
                HStack {
                    Spacer()
                    Button { store.terminalAction("new") } label: { Image(systemName: "plus") }.help("New terminal")
                }
                .buttonStyle(.borderless).padding(.horizontal, 8).padding(.vertical, 6)
                List(selection: Binding(get: { feed.state.active }, set: { if let id = $0 { store.terminalAction("activate", id: id) } })) {
                    ForEach(feed.state.tabs) { tab in
                        HStack {
                            Label(tab.title, systemImage: "terminal").lineLimit(1)
                            Spacer()
                            Button { if tab.busy { closing = tab.id } else { store.terminalAction("close", id: tab.id) } } label: { Image(systemName: "xmark") }
                                .buttonStyle(.borderless).help("Close terminal")
                        }
                        .tag(tab.id)
                    }
                }
                .listStyle(.sidebar)
            }
            .frame(width: 180)
        }
        .confirmationDialog(workspaceTerminalConfirmation().title, isPresented: Binding(get: { closing != nil }, set: { if !$0 { closing = nil } })) {
            Button(workspaceTerminalConfirmation().confirm, role: .destructive) { if let id = closing { store.terminalAction("close", id: id, confirmed: true) }; closing = nil }
            Button(workspaceTerminalConfirmation().cancel, role: .cancel) { closing = nil }
        } message: { Text(workspaceTerminalConfirmation().body) }
    }
}

struct TerminalResizeHandle: View {
    @Binding var height: Double
    let maximum: CGFloat
    @State private var start: Double?
    var body: some View {
        Divider()
            .overlay {
                Color.clear.frame(height: 7).contentShape(Rectangle())
                    .pointerStyle(.rowResize)
                    .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                        .onChanged { value in
                            let origin = start ?? min(height, maximum)
                            start = origin
                            height = min(max(100, origin - value.translation.height), maximum)
                        }
                        .onEnded { _ in start = nil })
            }
            .accessibilityLabel("Resize Terminal")
    }
}

private struct TerminalSurface: NSViewRepresentable {
    let store: WorkspaceStore
    let id: UInt64
    let frame: TerminalGrid?
    func makeNSView(context: Context) -> TerminalNativeView { TerminalNativeView() }
    func updateNSView(_ view: TerminalNativeView, context: Context) {
        let switched = view.id != id
        view.store = store; view.id = id; view.grid = frame
        view.needsDisplay = true
        if switched { view.selection = nil; view.lastSize = .zero; view.resizeGrid(); DispatchQueue.main.async { view.window?.makeFirstResponder(view) } }
    }
}

private final class TerminalNativeView: NSView, NSTextInputClient {
    weak var store: WorkspaceStore?
    var id: UInt64 = 0
    var grid: TerminalGrid?
    var selection: ClosedRange<Int>?
    private var anchor = 0
    var lastSize = NSSize.zero
    private var marked = NSAttributedString(string: "")
    private let font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
    // Indexed by bold | italic << 1; measured once rather than for every drawn cell.
    private lazy var fonts: [NSFont] = [font, NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask),
        NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask),
        NSFontManager.shared.convert(font, toHaveTrait: [.boldFontMask, .italicFontMask])]
    private lazy var cellWidth: CGFloat = ("M" as NSString).size(withAttributes: [.font: font]).width
    private let cellHeight: CGFloat = 18
    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }
    override func layout() { super.layout(); resizeGrid() }
    func resizeGrid() {
        guard id != 0 else { return }
        let size = NSSize(width: min(500, max(2, Int((bounds.width - 12) / cellWidth))), height: min(200, max(1, Int((bounds.height - 8) / cellHeight))))
        if size != lastSize { lastSize = size; store?.terminalResize(id, cols: Int(size.width), rows: Int(size.height)) }
    }
    private func color(_ rgb: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255, blue: CGFloat(rgb & 255) / 255, alpha: 1)
    }
    private func origin(_ column: Int, _ row: Int) -> NSPoint { NSPoint(x: 6 + CGFloat(column) * cellWidth, y: 4 + CGFloat(row) * cellHeight) }
    override func draw(_ dirtyRect: NSRect) {
        color(grid?.background ?? 0x191b1f).setFill(); dirtyRect.fill()
        guard let grid, grid.cols > 0 else { return }
        let first = max(0, Int((dirtyRect.minY - 4) / cellHeight)), last = min(grid.rows - 1, Int((dirtyRect.maxY - 4) / cellHeight))
        guard first <= last else { return }
        for row in first...last {
            let base = row * grid.cols
            guard base + grid.cols <= grid.cells.count else { break }
            // Fill runs of equal non-default backgrounds.
            var column = 0
            while column < grid.cols {
                let selected = selection?.contains(base + column) == true, background = grid.cells[base + column].background
                var end = column + 1
                while end < grid.cols, (selection?.contains(base + end) == true) == selected, grid.cells[base + end].background == background { end += 1 }
                if selected || background != grid.background {
                    (selected ? NSColor.selectedTextBackgroundColor : color(background)).setFill()
                    NSRect(origin: origin(column, row), size: NSSize(width: CGFloat(end - column) * cellWidth, height: cellHeight)).fill()
                }
                column = end
            }
            // Draw runs of single-width ASCII with one style; other glyphs keep their own cell.
            column = 0
            while column < grid.cols {
                let cell = grid.cells[base + column]
                if cell.text.isEmpty || cell.text == " " { column += 1; continue }
                var text = cell.text, end = column + 1
                if cell.width == 1 && cell.text.utf8.count == 1 {
                    while end < grid.cols {
                        let next = grid.cells[base + end]
                        guard next.width == 1, next.text.utf8.count == 1, next.foreground == cell.foreground, next.bold == cell.bold,
                              next.italic == cell.italic, next.underline == cell.underline else { break }
                        text += next.text; end += 1
                    }
                }
                let attributes: [NSAttributedString.Key: Any] = [.font: fonts[(cell.bold ? 1 : 0) | (cell.italic ? 2 : 0)],
                    .foregroundColor: color(cell.foreground), .underlineStyle: cell.underline ? NSUnderlineStyle.single.rawValue : 0]
                (text as NSString).draw(at: origin(column, row), withAttributes: attributes)
                column = end
            }
        }
        if let cursor = grid.cursor, window?.firstResponder === self {
            NSColor.white.setStroke()
            NSBezierPath(rect: NSRect(origin: origin(cursor[0], cursor[1]), size: NSSize(width: cellWidth, height: cellHeight))).stroke()
        }
    }
    private func index(_ event: NSEvent) -> Int {
        guard let grid else { return 0 }; let p = convert(event.locationInWindow, from: nil)
        return min(grid.rows - 1, max(0, Int((p.y - 4) / cellHeight))) * grid.cols + min(grid.cols - 1, max(0, Int((p.x - 6) / cellWidth)))
    }
    override func mouseDown(with event: NSEvent) { window?.makeFirstResponder(self); anchor = index(event); selection = nil; needsDisplay = true }
    override func mouseDragged(with event: NSEvent) { let end = index(event); selection = min(anchor, end)...max(anchor, end); needsDisplay = true }
    override func scrollWheel(with event: NSEvent) { store?.terminalScroll(id, lines: Int32(event.scrollingDeltaY > 0 ? -3 : 3)) }
    @objc func copy(_ sender: Any?) {
        guard let grid, let selection else { return }; var text = ""
        for i in selection where i < grid.cells.count {
            if i > selection.lowerBound && i % grid.cols == 0 { text += "\n" }
            if grid.cells[i].width > 0 { text += grid.cells[i].text.isEmpty ? " " : grid.cells[i].text }
        }
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(text, forType: .string)
    }
    @objc func paste(_ sender: Any?) { if let text = NSPasteboard.general.string(forType: .string) { store?.terminalInput(id, text: text, paste: true) } }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.contains(.command) {
            if event.charactersIgnoringModifiers == "c" { copy(nil); return true }
            if event.charactersIgnoringModifiers == "v" { paste(nil); return true }
        }
        return super.performKeyEquivalent(with: event)
    }
    override func keyDown(with event: NSEvent) {
        let names: [UInt16: String] = [36: "Enter", 48: "Tab", 51: "Backspace", 53: "Escape", 117: "Delete", 115: "Home", 119: "End", 116: "PageUp", 121: "PageDown", 123: "ArrowLeft", 124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp", 122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7", 100: "F8", 101: "F9", 109: "F10", 103: "F11", 111: "F12"]
        let flags = event.modifierFlags
        let modifiers: UInt16 = (flags.contains(.shift) ? 1 : 0) | (flags.contains(.control) ? 2 : 0) | (flags.contains(.option) ? 4 : 0)
        if let name = names[event.keyCode] { store?.terminalInput(id, key: name, modifiers: modifiers) }
        else if flags.contains(.control) || flags.contains(.option) {
            let text = event.charactersIgnoringModifiers ?? ""
            store?.terminalInput(id, key: text, text: text, modifiers: modifiers)
        } else { interpretKeyEvents([event]) }
    }
    func insertText(_ string: Any, replacementRange: NSRange) { let text = (string as? NSAttributedString)?.string ?? (string as? String ?? ""); marked = NSAttributedString(string: ""); store?.terminalInput(id, text: text) }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) { marked = string as? NSAttributedString ?? NSAttributedString(string: string as? String ?? "") }
    func unmarkText() { marked = NSAttributedString(string: "") }
    func selectedRange() -> NSRange { NSRange(location: NSNotFound, length: 0) }
    func markedRange() -> NSRange { NSRange(location: marked.length > 0 ? 0 : NSNotFound, length: marked.length) }
    func hasMarkedText() -> Bool { marked.length > 0 }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect { window?.convertToScreen(convert(NSRect(x: 6, y: 4, width: cellWidth, height: cellHeight), to: nil)) ?? .zero }
    func characterIndex(for point: NSPoint) -> Int { 0 }
    override func doCommand(by selector: Selector) { }
}
