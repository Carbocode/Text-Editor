import AppKit

enum WorkspaceIcons {
    static func image(for name: String) -> NSImage? {
        let iconName: String
        if name.lowercased() == "readme.md" {
            iconName = "readme"
        } else if name.lowercased().hasSuffix(".md") {
            iconName = "markdown"
        } else {
            iconName = "document"
        }
        guard let url = Bundle.main.url(forResource: iconName, withExtension: "png", subdirectory: "icons") else {
            return nil
        }
        return NSImage(contentsOf: url)
    }
}
