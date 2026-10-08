import AppKit

@main
struct IconSmoke {
    static func main() {
        guard let bundle = Bundle(path: "apps/macos/.build/NativeWorkspace.app"),
              let generic = WorkspaceIcons.resourceURL(for: "folder", in: bundle),
              let macOS = WorkspaceIcons.resourceURL(for: "folder-macos", in: bundle) else {
            fatalError("Icon bundle is unavailable")
        }
        precondition(generic.lastPathComponent == "folder.png")
        precondition(macOS.lastPathComponent == "folder-macos.png")
        precondition(FileManager.default.fileExists(atPath: generic.path))
        precondition(FileManager.default.fileExists(atPath: macOS.path))
        precondition((try? Data(contentsOf: generic)) != (try? Data(contentsOf: macOS)))
        print("macOS exact icon resource smoke test passed")
    }
}
