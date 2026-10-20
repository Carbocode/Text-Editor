import AppKit
import SwiftUI

@main
struct GitSmoke {
    @MainActor static func main() throws {
        setbuf(stdout, nil)
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        app.setValue(true, forKey: "accessibilityEnhancedUserInterface")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        func git(_ args: [String]) throws {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            process.arguments = ["git", "-C", root.path, "-c", "user.name=Test", "-c", "user.email=test@example.invalid"] + args
            process.standardOutput = FileHandle.nullDevice
            process.standardError = FileHandle.nullDevice
            try process.run()
            process.waitUntilExit()
            precondition(process.terminationStatus == 0)
        }
        let file = root.appendingPathComponent("note.txt")
        try "original\n".write(to: file, atomically: true, encoding: .utf8)
        try git(["init", "-q", "-b", "toolbar/a-long-branch-name-that-must-not-hide-the-toolbar"])
        try git(["add", "."])
        try git(["commit", "-qm", "Initial"])
        try "staged\n".write(to: file, atomically: true, encoding: .utf8)
        try git(["add", "note.txt"])
        try "working tree\n".write(to: file, atomically: true, encoding: .utf8)
        let core = WorkspaceApplication()
        let store = WorkspaceStore(core: core, persistence: false)
        store.openWorkspace(root)
        func finish() {
            let deadline = Date().addingTimeInterval(5)
            while store.backendBusy && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
            precondition(!store.backendBusy && store.errorMessage == nil, store.errorMessage ?? "Git worker did not finish")
        }
        finish()
        let window = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 1000, height: 700),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.contentView = NSHostingView(rootView: ContentView(store: store))
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        defer { window.orderOut(nil) }
        func wait(line: Int = #line, _ condition: () -> Bool) {
            let deadline = Date().addingTimeInterval(5)
            while !condition() && Date() < deadline {
                store.poll()
                RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            }
            if !condition() { print("Git sidebar timeout at line \(line): loaded=\(store.git.loaded), busy=\(store.backendBusy), actions=\(find("gitActions") != nil), tab=\(find("sidebarTab:git") != nil)") }
            precondition(condition(), "Git sidebar did not become ready: \(store.errorMessage ?? "")")
        }
        func element(_ identifier: String, in parent: Any) -> (any NSAccessibilityElementProtocol)? {
            if let accessible = parent as? any NSAccessibilityElementProtocol,
               accessible.accessibilityIdentifier?() == identifier { return accessible }
            if let object = parent as? NSObject,
               object.responds(to: NSSelectorFromString("accessibilityChildren")),
               let children = object.value(forKey: "accessibilityChildren") as? [Any] {
                for child in children {
                    if let found = element(identifier, in: child) { return found }
                }
            }
            return nil
        }
        func find(_ identifier: String) -> (any NSAccessibilityElementProtocol)? { element(identifier, in: window) }
        func click(_ identifier: String) {
            let frame = find(identifier)!.accessibilityFrame()
            let point = window.convertPoint(fromScreen: NSPoint(x: frame.midX, y: frame.midY))
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, eventNumber: 1, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
                window.sendEvent(event)
            }
        }
        wait { find("sidebarTab:git") != nil }
        precondition(!store.git.loaded, "Git should load through the actual sidebar tab")
        click("sidebarTab:git")
        wait { store.git.loaded && !store.backendBusy && find("gitActions") != nil }
        precondition(store.git.repository != nil && !store.git.history.isEmpty)
        func checkToolbar() {
            window.contentView?.layoutSubtreeIfNeeded()
            let panel = find("gitPanel")!.accessibilityFrame()
            let graph = find("gitGraph")!.accessibilityFrame()
            for identifier in ["gitBranch", "gitRefresh", "gitActions"] {
                let frame = find(identifier)!.accessibilityFrame()
                precondition(frame.width > 10 && frame.height > 10 && panel.insetBy(dx: -1, dy: -1).contains(frame),
                    "Git control \(identifier) is clipped: \(frame), panel: \(panel)")
                precondition(frame.minY > graph.maxY, "Git toolbar is covered by the graph")
            }
            precondition(abs(graph.height - panel.height / 2) < 35, "Git sidebar did not open evenly split")
        }
        checkToolbar()
        func splitView(in view: NSView) -> NSSplitView? {
            if let split = view as? NSSplitView { return split }
            return view.subviews.lazy.compactMap { splitView(in: $0) }.first
        }
        let sidebarSplit = splitView(in: window.contentView!)!
        sidebarSplit.setPosition(260, ofDividerAt: 0)
        wait { find("gitPanel")!.accessibilityFrame().width <= 261 }
        checkToolbar()
        sidebarSplit.setPosition(300, ofDividerAt: 0)
        click("sidebarTab:files")
        wait { find("gitActions") == nil }
        click("sidebarTab:git")
        wait { find("gitActions") != nil && !store.backendBusy }
        checkToolbar()
        try core.delayNextBackgroundJob(milliseconds: 150)
        precondition(store.openPaths([root]))
        wait { store.git.loaded && !store.backendBusy }
        checkToolbar()
        let change = store.git.repository?.changes.first
        precondition(change?.path == "note.txt" && change?.staged == true && change?.unstaged == true)
        store.showGitDiff("note.txt", staged: true)
        finish()
        precondition(store.git.diff?.text.contains("+staged") == true)
        store.showGitDiff("note.txt", staged: false)
        finish()
        precondition(store.git.diff?.text.contains("+working tree") == true)
        store.gitAction("identity", "Native Test", "native@example.invalid")
        finish()
        store.gitAction("stage")
        finish()
        store.gitAction("commit", "Native second commit")
        finish()
        let base = store.git.repository!.branch
        store.gitAction("branch", "feature", "HEAD")
        finish()
        try "feature\n".write(to: root.appendingPathComponent("feature.txt"), atomically: true, encoding: .utf8)
        store.gitAction("stage"); finish()
        store.gitAction("commit", "Feature commit"); finish()
        store.gitAction("checkout", base); finish()
        try "main\n".write(to: root.appendingPathComponent("main.txt"), atomically: true, encoding: .utf8)
        store.gitAction("stage"); finish()
        store.gitAction("commit", "Main commit"); finish()
        store.gitAction("merge", "feature"); finish()
        precondition(store.git.history.first?.commit.parents.count == 2)
        precondition(store.git.history.contains { $0.columns > 1 })
        precondition(store.git.graph_width == 2 && store.git.history.first?.merge == true && store.git.history.first?.head == true)
        precondition(store.git.history.first?.commit.refs.first?.current == true, "current branch badge missing")
        let merge = store.git.history.first!.commit.id
        store.gitAction("show_commit", merge); finish()
        precondition(store.git.inspection?.commit?.files.map(\.path) == ["feature.txt"], "commit detail files missing")
        store.gitAction("show_commit_file", merge, "feature.txt"); finish()
        precondition(store.git.patch_lines.contains { $0.kind == "added" && $0.new == 1 }, "numbered commit file diff missing")
        // Long references and subjects truncate inside the narrowest sidebar.
        store.gitAction("commit", String(repeating: "A very long commit subject ", count: 8), "", true); finish()
        store.gitAction("tag", "release-candidate-with-a-very-long-tag-name-0001", "HEAD"); finish()
        store.gitAction("tag", "second-tag", "HEAD"); finish()
        sidebarSplit.setPosition(260, ofDividerAt: 0)
        let head = store.git.history.first!.short_id
        wait { find("gitCommit:\(head)") != nil && find("gitPanel")!.accessibilityFrame().width <= 261 }
        window.contentView?.layoutSubtreeIfNeeded()
        let bounds = find("gitPanel")!.accessibilityFrame().insetBy(dx: -1, dy: -1)
        func frames(_ parent: Any) -> [NSRect] {
            var result: [NSRect] = []
            if let accessible = parent as? any NSAccessibilityElementProtocol { result.append(accessible.accessibilityFrame()) }
            if let object = parent as? NSObject, object.responds(to: NSSelectorFromString("accessibilityChildren")),
               let children = object.value(forKey: "accessibilityChildren") as? [Any] {
                result += children.flatMap(frames)
            }
            return result
        }
        var checked = 0
        for row in store.git.history {
            guard let element = find("gitCommit:\(row.short_id)") else { continue }
            // Every badge, subject and date inside the row, not only its frame.
            for frame in frames(element) where frame.width > 0 {
                checked += 1
                precondition(frame.minX >= bounds.minX && frame.maxX <= bounds.maxX,
                    "Commit row \(row.short_id) content overflows the sidebar: \(frame), panel: \(bounds)")
            }
        }
        precondition(checked > store.git.history.count, "Commit row contents are not inspectable")
        if let badge = find("gitRef:" + store.git.history.first!.commit.refs.first!.name) {
            precondition(badge.accessibilityFrame().maxX <= bounds.maxX, "Reference badge overflows the sidebar")
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.1))
        precondition(window.contentView?.fittingSize.width ?? 0 > 0)
        if let output = ProcessInfo.processInfo.environment["NATIVE_WORKSPACE_GIT_SCREENSHOT"],
           let view=window.contentView, let bitmap=view.bitmapImageRepForCachingDisplay(in:view.bounds) {
            view.cacheDisplay(in:view.bounds,to:bitmap)
            try bitmap.representation(using:.png,properties:[:])?.write(to:URL(fileURLWithPath:output))
        }
        store.refreshGit()
        finish()
        precondition(store.git.diff == nil)
        let saved = try String(contentsOf: file, encoding: .utf8)
        precondition(saved == "working tree\n")
        print("Git sidebar tab loading/visible toolbar/split graph and operations smoke passed")
    }
}
