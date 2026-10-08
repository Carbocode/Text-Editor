import AppKit

enum WorkspaceIcons {
    private static let cache = NSCache<NSString, NSImage>()

    private static let manifest: [String: Any] = {
        guard let url = Bundle.main.url(forResource: "manifest", withExtension: "json", subdirectory: "icons"),
              let data = try? Data(contentsOf: url),
              let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return [:]
        }
        return value
    }()

    private static let extensionsByLength: [String] = {
        let extensions = manifest["fileExtensions"] as? [String: String] ?? [:]
        return extensions.keys.sorted { $0.count > $1.count }
    }()

    static func image(for path: String, isDirectory: Bool = false, expanded: Bool = false) -> NSImage? {
        let icon = iconName(for: path, isDirectory: isDirectory, expanded: expanded)
        if let cached = cache.object(forKey: icon as NSString) { return cached }
        guard let url = Bundle.main.url(forResource: icon, withExtension: "png", subdirectory: "icons"),
              let image = NSImage(contentsOf: url) else { return nil }
        cache.setObject(image, forKey: icon as NSString)
        return image
    }

    private static func iconName(for path: String, isDirectory: Bool, expanded: Bool) -> String {
        let name = path.split(separator: "/").last.map(String.init)?.lowercased() ?? path.lowercased()
        if isDirectory {
            let key = expanded ? "folderNamesExpanded" : "folderNames"
            let fallback = expanded ? "folderExpanded" : "folder"
            return (manifest[key] as? [String: String])?[name]
                ?? (manifest[fallback] as? String) ?? "folder"
        }
        let names = manifest["fileNames"] as? [String: String] ?? [:]
        let lowerPath = path.lowercased()
        if let icon = names[lowerPath] ?? names[name] { return icon }
        let extensions = manifest["fileExtensions"] as? [String: String] ?? [:]
        for ext in extensionsByLength where name == ext || name.hasSuffix("." + ext) {
            return extensions[ext] ?? "file"
        }
        return (manifest["file"] as? String) ?? "file"
    }
}
