import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// File URLs are decoded by AppKit; validation and file operations stay in Rust.
struct ExternalFileDrop: DropDelegate {
    let entered: ([URL]) -> Bool
    let exited: () -> Void
    let dropped: ([URL]) -> Bool
    var operation: DropOperation = .move

    private func files(_ info: DropInfo) -> [URL] {
        guard info.hasItemsConforming(to: [.fileURL]) else { return [] }
        return (NSPasteboard(name: .drag).readObjects(forClasses: [NSURL.self], options: [
            .urlReadingFileURLsOnly: true
        ]) as? [URL]) ?? []
    }
    func validateDrop(info: DropInfo) -> Bool { !files(info).isEmpty }
    func dropEntered(info: DropInfo) { _ = entered(files(info)) }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: entered(files(info)) ? operation : .cancel)
    }
    func dropExited(info: DropInfo) { exited() }
    func performDrop(info: DropInfo) -> Bool { dropped(files(info)) }
}
