import AppKit

enum WorkspaceIcons {
    private static let cache = NSCache<NSString, NSImage>()

    static func image(for path: String, isDirectory: Bool = false, expanded: Bool = false) -> NSImage? {
        let icon = iconName(for: path, isDirectory: isDirectory, expanded: expanded)
        if let cached = cache.object(forKey: icon as NSString) { return cached }
        guard let url = resourceURL(for: icon, in: .main),
              let image = NSImage(contentsOf: url) else { return nil }
        cache.setObject(image, forKey: icon as NSString)
        return image
    }

    static func resourceURL(for icon: String, in bundle: Bundle) -> URL? {
        bundle.resourceURL?
            .appendingPathComponent("icons", isDirectory: true)
            .appendingPathComponent(icon + ".png", isDirectory: false)
    }

    private static func iconName(for path: String, isDirectory: Bool, expanded: Bool) -> String {
        isDirectory ? workspaceFolderIcon(value: path, expanded: expanded) : workspaceFileIcon(value: path)
    }
}
