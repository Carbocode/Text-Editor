import QtQuick
import QtQuick.Controls
import org.nativeworkspace.Diagnostic
import "../qml"

ApplicationWindow {
    width: 900
    height: 600
    visible: true

    AppController { id: controller }
    WorkspacePane {
        id: pane
        anchors.fill: parent
        controller: controller
    }
    FontMetrics { id: editorFontMetrics; font: pane.editorFont }
    FontInfo { id: resolvedEditorFont; font: pane.editorFont }

    property int step: 0
    property int treeResets: 0
    property var tree
    property var filesCache
    property var searchCache
    property var searchModel
    property int initialTreeResets: 0
    property string expandedPath: "src/main.rs"
    property real switchedAt: 0

    function check(condition, message) {
        if (!condition) throw new Error(message)
    }

    function findItem(item, name) {
        if (item.objectName === name) return item
        for (var child of item.children || []) {
            var found = findItem(child, name)
            if (found) return found
        }
        return null
    }

    function checkIcons() {
        var icons = []
        function collect(item) {
            if (item.objectName === "fileTreeIcon") icons.push(item)
            for (var child of item.children || []) collect(child)
        }
        collect(tree)
        check(icons.length > 0, "Files tree has no icon items")
        for (var icon of icons) {
            check(icon.status === Image.Ready, "Icon did not load: " + icon.source)
            check(icon.visible && icon.width === 16 && icon.height === 16,
                "Icon has no visible 16px area")
        }
        check(icons.some(icon => String(icon.source).endsWith("rust.png")),
            "Rust file did not use its bundled icon: " + icons.map(icon => String(icon.source)).join(", "))
        check(icons.some(icon => String(icon.source).endsWith("folder-src-open.png")),
            "Expanded folder did not use its open icon")
    }

    Component.onCompleted: {
        try {
            var argument = Qt.application.arguments.find(value => value.startsWith("--smoke-sidebar="))
            controller.open_workspace(argument.substring("--smoke-sidebar=".length))
            check(controller.error_message.length === 0, controller.error_message)
            pane.sidebarEntries()
            tree = findItem(pane, "fileTree")
            check(tree !== null, "Files tree not found")
            tree.model.modelReset.connect(function() { ++treeResets })
        } catch (error) {
            console.error(error)
            Qt.exit(1)
        }
    }

    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            try {
                switch (step++) {
                case 0:
                    check(Math.abs(editorFontMetrics.advanceWidth("iiiiiiii")
                        - editorFontMetrics.advanceWidth("WWWWWWWW")) < 0.1,
                        "Editor resolved to a proportional font")
                    console.log("KDE editor fixed font=" + resolvedEditorFont.family)
                    pane.toggleDirectory("src")
                    break
                case 1:
                    check(pane.treeRowForPath(expandedPath) >= 0, "Folder did not expand")
                    checkIcons()
                    filesCache = pane.sidebarCaches[0]
                    initialTreeResets = treeResets
                    pane.sidebarTab = 1
                    controller.search_files("sidebar needle")
                    pane.sidebarEntries()
                    break
                case 2:
                    if (controller.search_in_progress) {
                        --step
                        return
                    }
                    check(pane.sidebarEntries().some(entry => entry.path === expandedPath),
                        "Search did not show the content match")
                    searchCache = pane.sidebarCaches[1]
                    searchModel = findItem(pane, "searchResults").model
                    switchedAt = Date.now()
                    for (var round = 0; round < 20; ++round) {
                        for (var tab = 0; tab < 6; ++tab) {
                            pane.sidebarTab = tab
                            pane.sidebarEntries()
                            check(treeResets === initialTreeResets, "Tab switch reset the Files tree")
                            check(pane.sidebarCaches[0] === filesCache, "Tab switch rebuilt Files data")
                            check(pane.sidebarCaches[1] === searchCache, "Tab switch rebuilt Search data")
                            check(findItem(pane, "searchResults").model === searchModel,
                                "Tab switch replaced the Search model")
                            if (tab >= 2) check(pane.sidebarEntries().length === 0, "Empty tab kept stale rows")
                        }
                    }
                    console.log("KDE sidebar 120 cached tab switches=" + (Date.now() - switchedAt) + "ms")
                    pane.sidebarTab = 0
                    pane.sidebarEntries()
                    break
                case 3:
                    check(pane.treeRowForPath(expandedPath) >= 0, "Tab switch lost folder expansion")
                    check(pane.treeMatchesSidebarEntries(), "Cached tree and sidebar disagree")
                    checkIcons()
                    // Search updates while Files is active must not reset its tree.
                    controller.search_files("no-such-content")
                    check(pane.sidebarCaches[0] === filesCache, "Search invalidated Files cache")
                    pane.sidebarTab = 1
                    check(pane.sidebarEntries().length === 0, "Search cache was not invalidated")
                    // A name filter edited off-tab must invalidate only Files.
                    pane.fileNameQuery = "main"
                    pane.sidebarTab = 0
                    check(pane.sidebarEntries().some(entry => entry.path === expandedPath),
                        "Off-tab name filter did not find the file")
                    check(!pane.sidebarEntries().some(entry => entry.path === "README.md"),
                        "Off-tab name filter kept an unrelated file")
                    break
                case 4:
                    check(pane.treeMatchesSidebarEntries(), "Filtered native tree disagrees with sidebar")
                    controller.create_entry("src/new.rs", false)
                    pane.fileNameQuery = ""
                    pane.sidebarEntries()
                    break
                case 5:
                    check(pane.treeRowForPath("src/new.rs") >= 0, "New file did not update Files tree")
                    pane.toggleDirectory("src")
                    break
                case 6:
                    check(pane.treeRowForPath(expandedPath) === -1, "Folder did not collapse")
                    filesCache = pane.sidebarCaches[0]
                    pane.sidebarTab = 2
                    pane.sidebarEntries()
                    pane.sidebarTab = 0
                    pane.sidebarEntries()
                    check(pane.sidebarCaches[0] === filesCache, "Collapsed cache was rebuilt")
                    break
                case 7:
                    check(pane.treeRowForPath(expandedPath) === -1, "Tab switch lost collapsed state")
                    console.log("KDE sidebar tab/cache/icon smoke test passed")
                    Qt.quit()
                }
            } catch (error) {
                console.error(error)
                Qt.exit(1)
            }
        }
    }
}
