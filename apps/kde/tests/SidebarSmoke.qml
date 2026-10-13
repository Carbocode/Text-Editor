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

    function findItem(item, name, visited) {
        visited = visited || new Set()
        if (!item || visited.has(item)) return null
        visited.add(item)
        if (item.objectName === name && (name !== "sidebarRenameField" || item.visible)) return item
        var candidates = [...(item.children || []), ...(item.resources || [])]
        if (item.contentItem) candidates.push(item.contentItem)
        for (var child of candidates) {
            var found = findItem(child, name, visited)
            if (found) return found
        }
        return null
    }

    function checkNumberAlignment(lines) {
        var editor = findItem(pane, "documentEditor")
        var scroll = findItem(pane, "editorScroll")
        var gutter = findItem(pane, "editorLineNumbers")
        for (var line of lines) {
            var rect = editor.positionToRectangle(gutter.lineStarts[line])
            var numberCenter = gutter.numberBaseline(line)
                - (editorFontMetrics.ascent - editorFontMetrics.descent) / 2
            check(Math.abs(numberCenter - (rect.y + rect.height / 2 - scroll.contentItem.contentY)) < 0.1,
                "Line number " + (line + 1) + " is not vertically centered")
        }
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
                    controller.open_document("src/new.rs")
                    pane.editorText = "first\nsecond\n"
                    break
                case 8:
                    var editor = findItem(pane, "documentEditor")
                    var scroll = findItem(pane, "editorScroll")
                    var activeLine = findItem(pane, "editorActiveLine")
                    check(editor.width >= scroll.availableWidth, "Short text does not fill editor width")
                    check(editor.height >= scroll.availableHeight, "Short text does not fill editor height")
                    var lastY = editor.positionToRectangle(editor.length).y
                    editor.cursorPosition = editor.positionAt(scroll.availableWidth - 5,
                        lastY + editor.cursorRectangle.height / 2)
                    check(editor.cursorPosition === editor.length, "Right edge cannot reach final empty line")
                    check(activeLine.y === editor.cursorRectangle.y - scroll.contentItem.contentY
                        && activeLine.height === editor.cursorRectangle.height,
                        "Active line does not follow final empty line")
                    check(activeLine.width === editor.width, "Active line does not fill editor width")
                    checkNumberAlignment([0, 1, 2])
                    editor.cursorPosition = 0
                    check(activeLine.y < lastY, "Active line did not follow cursor navigation")
                    editor.text = ""
                    break
                case 9:
                    var emptyEditor = findItem(pane, "documentEditor")
                    var emptyScroll = findItem(pane, "editorScroll")
                    check(emptyEditor.height >= emptyScroll.availableHeight, "Empty document lost clickable viewport")
                    check(emptyEditor.positionAt(emptyScroll.availableWidth - 5,
                        emptyEditor.topPadding + emptyEditor.cursorRectangle.height / 2) === 0,
                        "Empty line cannot be clicked at right edge")
                    console.log("KDE editor full-width/active-line smoke test passed")
                    emptyEditor.text = "filler\n".repeat(100)
                    emptyEditor.cursorPosition = 7 * 40
                    break
                case 10:
                    var scroll = findItem(pane, "editorScroll")
                    scroll.contentItem.contentY = editorFontMetrics.height * 20
                    break
                case 11:
                    var editor = findItem(pane, "documentEditor")
                    var scroll = findItem(pane, "editorScroll")
                    var activeLine = findItem(pane, "editorActiveLine")
                    var gutter = findItem(pane, "editorLineNumbers")
                    check(scroll.contentItem.contentY > 0, "Editor did not scroll")
                    checkNumberAlignment([20, 40, 100])
                    var expectedY = editor.cursorRectangle.y - scroll.contentItem.contentY
                    check(Math.abs(activeLine.mapToItem(gutter, 0, 0).y - expectedY) < 1,
                        "Text highlight does not align with gutter after scrolling")
                    scroll.contentItem.contentY += editorFontMetrics.height * 5
                    break
                case 12:
                    var editor = findItem(pane, "documentEditor")
                    var scroll = findItem(pane, "editorScroll")
                    var activeLine = findItem(pane, "editorActiveLine")
                    var gutter = findItem(pane, "editorLineNumbers")
                    check(Math.abs(activeLine.mapToItem(gutter, 0, 0).y
                        - (editor.cursorRectangle.y - scroll.contentItem.contentY)) < 1,
                        "Text highlight stayed fixed after a second scroll")
                    check(activeLine.parent.clip, "Offscreen highlight is not clipped to the viewport")
                    checkNumberAlignment([25, 40, 100])
                    console.log("KDE editor scrolling active-line smoke test passed")
                    pane.editorText = "unsaved rename"
                    pane.beginRename("src")
                    break
                case 13:
                    var renameField = findItem(pane, "sidebarRenameField")
                    check(renameField && renameField.visible, "Inline rename field is missing")
                    check(renameField.selectedText === "src", "Folder name was not selected")
                    renameField.text = "renamed"
                    pane.renameName = renameField.text
                    pane.commitRename()
                    break
                case 14:
                    check(controller.error_message.length === 0, "Folder rename failed")
                    check(controller.active_document_id === "renamed/new.rs", "Rename lost active document")
                    check(controller.active_document_text === "unsaved rename" && controller.active_document_dirty,
                        "Rename lost unsaved edits")
                    console.log("KDE inline rename/buffer smoke test passed")
                    controller.create_entry("destination", true)
                    pane.editorText = "unsaved move"
                    pane.startEntryDrag("renamed", Qt.point(30, 30))
                    check(findItem(pane, "sidebarDragGhost").visible, "Drag ghost is missing")
                    check(!pane.canDrop("renamed", "renamed"), "Folder accepted a drop inside itself")
                    check(!pane.canDrop("README.md", ""), "Same-parent drop should be forbidden")
                    pane.hoverDrop("destination")
                    check(pane.collapsedDirectories["destination"] !== false, "Folder expanded before hover delay")
                    switchedAt = Date.now()
                    break
                case 15:
                    if (Date.now() - switchedAt < 750) { --step; break }
                    check(pane.collapsedDirectories["destination"] === false, "Hover did not expand closed folder")
                    check(pane.dropEntry("destination"), "Folder drop failed")
                    pane.finishEntryDrag(true)
                    break
                case 16:
                    check(controller.active_document_id === "destination/renamed/new.rs", "Move lost active document")
                    check(controller.active_document_text === "unsaved move" && controller.active_document_dirty,
                        "Move lost unsaved edits")
                    pane.startEntryDrag("destination/renamed/new.rs", Qt.point(30, 30))
                    check(pane.dropEntry(""), "Drop to root failed")
                    pane.finishEntryDrag(true)
                    controller.save_document()
                    check(controller.error_message.length === 0 && !controller.active_document_dirty,
                        "Could not save moved document")
                    pane.beginRename("new.rs")
                    break
                case 17:
                    var fileRename = findItem(pane, "sidebarRenameField")
                    check(fileRename && fileRename.selectedText === "new", "File rename should select its stem")
                    fileRename.text = "renamed.rs"
                    pane.renameName = fileRename.text
                    pane.commitRename()
                    break
                case 18:
                    check(controller.active_document_id === "renamed.rs", "Inline file rename failed")
                    check(controller.active_document_text === "unsaved move", "Inline rename lost content")
                    console.log("KDE drag ghost/drop/hover expansion smoke test passed")
                    Qt.quit()
                }
            } catch (error) {
                console.error(error)
                Qt.exit(1)
            }
        }
    }
}
