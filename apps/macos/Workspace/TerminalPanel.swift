import AppKit
import SwiftUI

struct TerminalPanelState: Decodable {
    struct Tab: Decodable, Identifiable { var id: UInt64; var title: String; var running: Bool; var status: String }
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
    @State private var closing: UInt64?
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                ScrollView(.horizontal) {
                    HStack {
                        ForEach(store.terminals.tabs) { tab in
                            HStack(spacing: 6) {
                                Button(tab.title) { store.terminalAction("activate", id: tab.id) }
                                    .fontWeight(store.terminals.active == tab.id ? .semibold : .regular)
                                Button { if tab.running { closing = tab.id } else { store.terminalAction("close", id: tab.id) } } label: { Image(systemName: "xmark") }
                                    .help("Close terminal")
                            }
                            .padding(7)
                            .background(store.terminals.active == tab.id ? Color.primary.opacity(0.08) : Color.clear)
                        }
                    }
                }
                Button { store.terminalAction("new") } label: { Image(systemName: "plus") }.help("New terminal")
                Button { store.terminalVisible = false } label: { Image(systemName: "chevron.down") }.help("Hide terminal")
            }.buttonStyle(.plain).padding(.horizontal, 8)
            Divider()
            if let id = store.terminals.active {
                TerminalSurface(store: store, id: id, frame: store.terminals.frame)
                if let tab = store.terminals.tabs.first(where: { $0.id == id }), !tab.status.isEmpty {
                    Text(tab.status).font(.caption).padding(4)
                }
            } else {
                Button("New Terminal") { store.terminalAction("new") }.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .confirmationDialog(workspaceTerminalConfirmation().title, isPresented: Binding(get: { closing != nil }, set: { if !$0 { closing = nil } })) {
            Button(workspaceTerminalConfirmation().confirm, role: .destructive) { if let id = closing { store.terminalAction("close", id: id, confirmed: true) }; closing = nil }
            Button(workspaceTerminalConfirmation().cancel, role: .cancel) { closing = nil }
        } message: { Text(workspaceTerminalConfirmation().body) }
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
    private var cellWidth: CGFloat { ("M" as NSString).size(withAttributes: [.font: font]).width }
    private let cellHeight: CGFloat = 18
    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }
    override func layout() { super.layout(); resizeGrid() }
    func resizeGrid() {
        guard id != 0 else { return }
        let size = NSSize(width: min(500, max(2, Int((bounds.width - 12) / cellWidth))), height: min(200, max(1, Int((bounds.height - 8) / cellHeight))))
        if size != lastSize { lastSize = size; store?.terminalResize(id, cols: Int(size.width), rows: Int(size.height)) }
    }
    private func color(_ rgb: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255, blue: CGFloat(rgb & 255) / 255, alpha: 1)
    }
    override func draw(_ dirtyRect: NSRect) {
        color(grid?.background ?? 0x191b1f).setFill(); bounds.fill()
        guard let grid else { return }
        for (index, cell) in grid.cells.enumerated() {
            let rect = NSRect(x: 6 + CGFloat(index % grid.cols) * cellWidth, y: 4 + CGFloat(index / grid.cols) * cellHeight, width: cellWidth, height: cellHeight)
            (selection?.contains(index) == true ? NSColor.selectedTextBackgroundColor : color(cell.background)).setFill(); rect.fill()
        }
        for (index, cell) in grid.cells.enumerated() where !cell.text.isEmpty {
            var cellFont = cell.bold ? NSFontManager.shared.convert(font, toHaveTrait: .boldFontMask) : font
            if cell.italic { cellFont = NSFontManager.shared.convert(cellFont, toHaveTrait: .italicFontMask) }
            let attrs: [NSAttributedString.Key: Any] = [.font: cellFont, .foregroundColor: color(cell.foreground), .underlineStyle: cell.underline ? NSUnderlineStyle.single.rawValue : 0]
            (cell.text as NSString).draw(at: NSPoint(x: 6 + CGFloat(index % grid.cols) * cellWidth, y: 4 + CGFloat(index / grid.cols) * cellHeight), withAttributes: attrs)
        }
        if let cursor = grid.cursor, window?.firstResponder === self {
            NSColor.white.setStroke()
            NSBezierPath(rect: NSRect(x: 6 + CGFloat(cursor[0]) * cellWidth, y: 4 + CGFloat(cursor[1]) * cellHeight, width: cellWidth, height: cellHeight)).stroke()
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
