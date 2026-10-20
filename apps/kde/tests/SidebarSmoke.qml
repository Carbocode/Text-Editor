import QtQuick
import org.nativeworkspace.Desktop
import "SmokeLifecycle.js" as SmokeLifecycle
import QtQuick.Controls
import QtTest
import org.kde.kirigami as Kirigami
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
    TestCase { id: input; when: false }
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
    property real nameFilterTypedAt: 0
    property real switchedAt: 0
    property bool waitingMoveSave: false
    property bool nativeEscapeChecked: false
    property bool screenshotPending: false
    property bool screenshotCaptured: false

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
        if (item.background) candidates.push(item.background)
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

    function checkFileRows() {
        for (var path of ["src", expandedPath]) {
            var row = tree.itemAtCell(Qt.point(0, pane.treeRowForPath(path)))
            check(row && row.contentItem && row.contentItem.title === path.split("/").pop(),
                "Native file row did not show its name: " + path)
            var expectedIcon = path === "src" ? controller.folder_icon(path, row.expanded) : controller.file_icon(path)
            check(row.icon.source.toString() === expectedIcon
                && row.contentItem.icon.source.toString() === expectedIcon,
                "File row did not use its Material icon: " + path)
            check(row.palette.base.toString() === row.Kirigami.Theme.backgroundColor.toString()
                && row.palette.alternateBase.toString() === row.Kirigami.Theme.alternateBackgroundColor.toString(),
                "Native file row did not use the Kirigami view palette: " + path)
            check(row.contentItem.color.toString() !== row.background.color.toString(),
                "Native file name is invisible: " + path)
            check(row.height >= row.contentItem.implicitHeight,
                "Native row clipped its label: " + path)
            check(row.height <= row.contentItem.implicitHeight + Kirigami.Units.largeSpacing * 2,
                "Desktop file rows are too tall: " + row.height)
        }
    }

    function checkSearchRows() {
        var list = findItem(pane, "searchResults")
        list.forceLayout()
        var folder = findItem(pane, "sidebarSearchFile:src")
        check(folder, "Search did not render the parent folder")
        check(findItem(pane, "sidebarSearchFile:" + expandedPath), "Search did not render the matching file")
        for (var entry of pane.sidebarEntries()) {
            var row = findItem(pane, "sidebarSearchFile:" + entry.path)
            if (!row) continue // Only visible ListView delegates are instantiated.
            var depth = entry.path.split("/").length - 1
            var left = row.contentItem.mapToItem(list, 0, 0).x
            var folderLeft = folder.contentItem.mapToItem(list, 0, 0).x
            check(Math.abs(left - folderLeft - depth * Kirigami.Units.gridUnit) <= 1,
                "Search row is not indented under its parent: " + entry.path)
            for (var index of pane.matchIndicesForFile(entry.path)) {
                var match = findItem(pane, "sidebarSearchMatch:" + index)
                check(match, "Search occurrence is missing under " + entry.path)
                check(Math.abs(match.contentItem.mapToItem(list, 0, 0).x - left
                    - Kirigami.Units.gridUnit) <= 1,
                    "Search occurrence is not indented under its file: " + entry.path)
            }
        }
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
            SmokeLifecycle.quit(controller, ApplicationLifecycle, 1)
        }
    }

    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            if (screenshotPending) return
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
                    checkFileRows()
                    var capture = Qt.application.arguments.find(argument => argument.startsWith("--smoke-screenshot="))
                    if (capture && !screenshotCaptured) {
                        screenshotPending = true
                        pane.grabToImage(function(result) {
                            screenshotCaptured = result.saveToFile(capture.substring("--smoke-screenshot=".length))
                            screenshotPending = false
                            if (!screenshotCaptured) SmokeLifecycle.quit(controller, ApplicationLifecycle, 1)
                        })
                        --step
                        break
                    }
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
                    checkSearchRows()
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
                    // Git polling can own the worker after the timed tab loop.
                    if (controller.move_pending) { --step; break }
                    if (nameFilterTypedAt === 0) {
                        check(pane.treeRowForPath(expandedPath) >= 0, "Tab switch lost folder expansion")
                        check(pane.treeMatchesSidebarEntries(), "Cached tree and sidebar disagree")
                        checkFileRows()
                        // Search updates while Files is active must not reset its tree.
                        controller.search_files("no-such-content")
                        check(pane.sidebarCaches[0] === filesCache, "Search invalidated Files cache")
                        pane.sidebarTab = 1
                        check(pane.sidebarEntries().length === 0, "Search cache was not invalidated")
                        // A name filter edited off-tab must invalidate only Files.
                        // Typing only queues it; the worker publishes the matches.
                        pane.fileNameQuery = "main"
                        check(pane.fileFilter === "" && controller.file_filter_in_progress,
                            "Typing a name filter must not filter on the UI thread")
                        nameFilterTypedAt = Date.now()
                    }
                    if (pane.fileFilter !== "main") {
                        check(Date.now() - nameFilterTypedAt < 3000, "Name filter results never arrived")
                        --step
                        break
                    }
                    check(!controller.file_filter_in_progress, "Published name filter still pending")
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
                    check(pane.fileFilter === "", "Clearing the name filter must apply at once")
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
                    check(pane.treeRowForPath(expandedPath) === -1, "Tab switch lost collapsed state: " + JSON.stringify(pane.collapsedDirectories) + " filter=" + pane.fileFilter + " restoring=" + pane.restoringFileTree)
                    console.log("KDE sidebar tab/cache/native row smoke test passed")
                    controller.open_document("src/new.rs")
                    pane.editorText = "first\nsecond\n"
                    break
                case 8:
                    var editor = findItem(pane, "documentEditor")
                    var scroll = findItem(pane, "editorScroll")
                    var activeLine = pane.editorActiveLine
                    check(editor.width >= scroll.availableWidth, "Short text does not fill editor width")
                    check(editor.height >= scroll.availableHeight, "Short text does not fill editor height")
                    var lastY = editor.positionToRectangle(editor.length).y
                    editor.cursorPosition = editor.positionAt(scroll.availableWidth - 5,
                        lastY + editor.cursorRectangle.height / 2)
                    check(editor.cursorPosition === editor.length, "Right edge cannot reach final empty line")
                    var gutter = findItem(pane, "editorLineNumbers")
                    check(Math.abs(activeLine.mapToItem(gutter, 0, 0).y
                        - (editor.cursorRectangle.y - scroll.contentItem.contentY)) < 1
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
                    var activeLine = pane.editorActiveLine
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
                    var activeLine = pane.editorActiveLine
                    var gutter = findItem(pane, "editorLineNumbers")
                    check(Math.abs(activeLine.mapToItem(gutter, 0, 0).y
                        - (editor.cursorRectangle.y - scroll.contentItem.contentY)) < 1,
                        "Text highlight stayed fixed after a second scroll")
                    check(activeLine.parent.clip, "Offscreen highlight is not clipped to the viewport")
                    checkNumberAlignment([25, 40, 100])
                    console.log("KDE editor scrolling active-line smoke test passed")
                    pane.editorText = "unsaved rename"
                    tree.forceLayout()
                    var folderDelegate = tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src")))
                    check(folderDelegate, "Folder row is missing for selected-click rename")
                    folderDelegate.clicked()
                    check(pane.selectedDirectoryPath === "src" && pane.renamePath.length === 0,
                        "First folder click must select it")
                    folderDelegate.clicked()
                    check(pane.renamePath.length === 0, "Selected folder click must not start renaming")
                    switchedAt = Date.now()
                    pane.beginRename("src")
                    break
                case 13:
                    var renameField = pane.activeRenameEditor
                    if ((!renameField || !renameField.visible) && Date.now() - switchedAt < 3000) { --step; break }
                    check(renameField && renameField.visible, "Inline rename field is missing: path=" + pane.renamePath + ", treeRows=" + tree.rows)
                    check(renameField.selectedText === "src", "Folder name was not selected: text=" + renameField.text + ", selected=" + renameField.selectedText + ", focus=" + renameField.activeFocus)
                    var renameRow = tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src")))
                    var renameTopLeft = renameField.mapToItem(renameRow, 0, 0)
                    check(renameTopLeft.y >= 0 && renameTopLeft.y + renameField.height <= renameRow.height,
                        "Inline rename extends outside its row vertically")
                    check(renameTopLeft.x >= 0 && renameTopLeft.x + renameField.width <= renameRow.width,
                        "Inline rename extends outside its row horizontally")
                    check(Math.abs(renameTopLeft.y + renameField.height / 2 - renameRow.height / 2) < 0.5,
                        "Inline rename is not centered inside its row")
                    renameField.text = "renamed"
                    input.keyClick(Qt.Key_Return)
                    break
                case 14:
                    check(controller.error_message.length === 0, "Folder rename failed: " + controller.error_message)
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
                    if (Date.now() - switchedAt < 500) {
                        pane.leaveDrop("destination")
                        pane.hoverDrop("destination")
                        --step
                        break
                    }
                    check(pane.collapsedDirectories["destination"] === false, "Hover did not expand closed folder")
                    check(pane.dropEntry("destination"), "Folder drop failed")
                    pane.finishEntryDrag(true)
                    break
                case 16:
                    // The drop commits on the next turn; wait for its confirmation.
                    if (controller.move_pending) {
                        controller.poll()
                        --step
                        break
                    }
                    check(controller.active_document_id === "destination/renamed/new.rs", "Move lost active document")
                    check(controller.active_document_text === "unsaved move" && controller.active_document_dirty,
                        "Move lost unsaved edits")
                    pane.startEntryDrag("destination/renamed/new.rs", Qt.point(30, 30))
                    check(pane.dropEntry(""), "Drop to root failed")
                    pane.finishEntryDrag(true)
                    controller.save_document()
                    waitingMoveSave = true
                    switchedAt = Date.now()
                    break
                case 17:
                    if (waitingMoveSave) {
                        controller.poll()
                        if (controller.active_document_dirty && Date.now() - switchedAt < 3000) {
                            --step
                            break
                        }
                        check(controller.error_message.length === 0 && !controller.active_document_dirty,
                            "Could not save moved document after background confirmation")
                        waitingMoveSave = false
                        tree.forceLayout()
                        var fileRow = pane.treeRowForPath("new.rs")
                        var fileDelegate = tree.itemAtCell(Qt.point(0, fileRow))
                        check(fileDelegate, "File row is missing for native rename")
                        pane.selectedDirectoryPath = "destination"
                        input.mouseClick(fileDelegate, fileDelegate.width / 2, fileDelegate.height / 2)
                        check(pane.renamePath.length === 0, "First file click must only select/open it")
                        input.mouseClick(fileDelegate, fileDelegate.width / 2, fileDelegate.height / 2)
                        check(pane.renamePath.length === 0, "Clicking an open file must not start renaming")
                        input.keyClick(Qt.Key_F2)
                        check(pane.renamePath === "new.rs", "F2 did not open the native edit delegate: focus="
                            + activeFocusItem + ", row=" + tree.currentRow + ", index=" + tree.selectionModel.currentIndex)
                        --step
                        break
                    }
                    var fileRename = pane.activeRenameEditor
                    check(fileRename && fileRename.selectedText === "new", "File rename should select its stem: field=" + fileRename + (fileRename ? ", text=" + fileRename.text + ", selected=" + fileRename.selectedText + ", focus=" + fileRename.activeFocus : ""))
                    if (!nativeEscapeChecked) {
                        fileRename.text = "discarded.rs"
                        input.keyClick(Qt.Key_Escape)
                        check(pane.renamePath.length === 0 && controller.active_document_id === "new.rs",
                            "Escape did not cancel the native rename")
                        nativeEscapeChecked = true
                        input.keyClick(Qt.Key_F2)
                        --step
                        break
                    }
                    fileRename.text = "renamed.rs"
                    input.keyClick(Qt.Key_Return)
                    break
                case 18:
                    check(controller.active_document_id === "renamed.rs", "Inline file rename failed")
                    check(controller.active_document_text === "unsaved move", "Inline rename lost content")
                    check(pane.collapsedDirectories["destination"] === false, "Drop did not promote folder expansion")
                    controller.create_entry("sibling", true)
                    pane.startEntryDrag("renamed.rs", Qt.point(30, 30))
                    pane.hoverDrop("destination/renamed")
                    switchedAt = Date.now()
                    break
                case 19:
                    if (Date.now() - switchedAt < 500) { --step; break }
                    check(pane.collapsedDirectories["destination/renamed"] === false, "Nested folder did not open temporarily")
                    pane.hoverDrop("sibling")
                    check(pane.collapsedDirectories["destination/renamed"] !== false, "Sibling hover did not restore the previous branch")
                    check(pane.collapsedDirectories["destination"] === false, "Pre-existing expansion must be retained")
                    switchedAt = Date.now()
                    break
                case 20:
                    if (Date.now() - switchedAt < 500) { --step; break }
                    check(pane.collapsedDirectories["sibling"] === false, "Sibling folder did not open temporarily")
                    pane.finishEntryDrag(true)
                    check(pane.collapsedDirectories["sibling"] !== false, "Cancelled drag retained a temporary expansion")
                    check(pane.collapsedDirectories["destination"] === false, "Cancellation lost a confirmed expansion")
                    console.log("KDE drag ghost/drop/hover expansion/sibling rollback smoke test passed")
                    pane.sidebarTab = 1
                    controller.search_files("unsaved move")
                    switchedAt = Date.now()
                    break
                case 21:
                    if (controller.search_in_progress) { --step; break }
                    findItem(pane, "searchResults").forceLayout()
                    var searchFile = findItem(pane, "sidebarSearchFile:renamed.rs")
                    if (!searchFile && Date.now() - switchedAt < 3000) { --step; break }
                    check(searchFile, "Search file row is missing for selected-click rename: results=" + JSON.stringify(controller.search_results) + ", entries=" + JSON.stringify(pane.sidebarEntries()) + ", query=" + controller.search_query + ", error=" + controller.error_message + ", list count=" + findItem(pane, "searchResults").count)
                    searchFile.clicked()
                    check(pane.renamePath.length === 0, "Clicking a search file must not start renaming")
                    pane.beginRename("renamed.rs")
                    break
                case 22:
                    var searchRename = pane.activeRenameEditor
                    check(searchRename && searchRename.visible && searchRename.activeFocus,
                        "Selected search file click did not focus the inline rename editor")
                    check(searchRename.selectedText === "renamed", "Search rename should select the file stem")
                    input.mouseClick(searchRename, searchRename.width / 2, searchRename.height / 2)
                    check(pane.renamePath === "renamed.rs", "Click inside the input cancelled rename")
                    searchRename.text = "discarded.rs"
                    pane.renameName = searchRename.text
                    var documentEditor = findItem(pane, "documentEditor")
                    input.mouseClick(documentEditor, documentEditor.leftPadding + 10, documentEditor.topPadding + 5)
                    check(documentEditor.activeFocus, "Click outside the rename field did not reach the editor")
                    check(pane.renamePath.length === 0, "Native focus loss did not cancel rename")
                    check(controller.document_ids.indexOf("renamed.rs") !== -1
                        && controller.document_ids.indexOf("discarded.rs") === -1,
                        "Outside click applied the drafted file name")
                    console.log("KDE Files/Search native rename/stem selection smoke test passed")
                    documentEditor.text = "unsaved deletion"
                    check(pane.flushEditor(), "Could not flush edits before deletion")
                    pane.requestDelete("renamed.rs")
                    var deleteDialog = findItem(pane, "deleteEntryDialog")
                    check(deleteDialog && deleteDialog.visible, "Deletion confirmation did not open")
                    deleteDialog.close()
                    check(controller.document_ids.indexOf("renamed.rs") !== -1, "Cancel removed a file")
                    pane.requestDelete("renamed.rs")
                    deleteDialog.customFooterActions[1].trigger()
                    check(controller.editing_paused, "Deletion did not start in the background")
                    switchedAt = Date.now()
                    break
                case 23:
                    if (controller.editing_paused && Date.now() - switchedAt < 3000) { --step; break }
                    check(!controller.editing_paused, "Deletion never finished")
                    check(controller.error_message.indexOf("Save all modified") !== -1, "Dirty deletion was not rejected")
                    check(controller.active_document_text === "unsaved deletion", "Deletion lost unsaved edits")
                    controller.save_document()
                    pane.requestDelete("renamed.rs")
                    findItem(pane, "deleteEntryDialog").customFooterActions[1].trigger()
                    switchedAt = Date.now()
                    break
                case 24:
                    if (controller.editing_paused && Date.now() - switchedAt < 3000) { --step; break }
                    check(!controller.editing_paused && controller.error_message.length === 0, "File deletion failed")
                    check(controller.document_ids.indexOf("renamed.rs") === -1 && controller.open_document_ids.indexOf("renamed.rs") === -1 && controller.active_document_id !== "renamed.rs",
                        "Deleted document stayed in the editor or tree")
                    controller.create_entry("delete-check", true)
                    controller.create_entry("delete-check/note", false)
                    pane.requestDelete("delete-check")
                    findItem(pane, "deleteEntryDialog").customFooterActions[1].trigger()
                    switchedAt = Date.now()
                    break
                case 25:
                    if (controller.editing_paused && Date.now() - switchedAt < 3000) { --step; break }
                    check(!controller.editing_paused && controller.error_message.length === 0, "Folder deletion failed")
                    check(controller.directory_ids.indexOf("delete-check") === -1
                        && controller.document_ids.indexOf("delete-check/note") === -1, "Deleted subtree stayed in the tree")
                    console.log("KDE deletion confirmation/cancel/unsaved protection/file/folder smoke test passed")
                    controller.create_entry("tab-review-a.txt", false)
                    controller.create_entry("tab-review-b.txt", false)
                    controller.open_document("tab-review-b.txt")
                    var tabEditor = findItem(pane, "documentEditor")
                    tabEditor.text = "edited"
                    check(pane.flushEditor(), "Could not flush tab fixture")
                    controller.save_all_documents()
                    controller.open_document("tab-review-a.txt")
                    tabEditor.insert(0, "edited")
                    check(pane.flushEditor() && tabEditor.canUndo, "Fixture must contain an undoable edit")
                    controller.open_document("tab-review-b.txt")
                    check(!tabEditor.canUndo, "Undo history leaked between equal tab buffers")
                    tabEditor.insert(tabEditor.length, "!")
                    check(pane.flushEditor() && tabEditor.canUndo, "Second tab edit must be undoable")
                    tabEditor.select(1, 3)
                    controller.rename_entry("tab-review-b.txt", "tab-review-renamed.txt")
                    check(tabEditor.canUndo && tabEditor.selectedText === "di", "Rename lost undo history or selection")
                    controller.save_all_documents()
                    switchedAt = Date.now()
                    break
                case 26:
                    var tabEditor = findItem(pane, "documentEditor")
                    if (controller.move_pending && Date.now() - switchedAt < 3000) { --step; break }
                    check(!controller.move_pending, "Queued Save All did not finish")
                    check(controller.dirty_document_ids.length === 0, "Save All did not save inactive documents")
                    controller.open_document("tab-review-a.txt")
                    tabEditor.insert(tabEditor.length, " discard")
                    pane.requestCloseDocument("tab-review-a.txt")
                    var closeDialog = findItem(pane, "closeDocumentDialog")
                    check(closeDialog && closeDialog.visible, "Dirty tab close did not ask for confirmation")
                    closeDialog.customFooterActions[0].trigger()
                    check(controller.open_document_ids.indexOf("tab-review-a.txt") !== -1, "Cancel closed a dirty tab")
                    pane.requestCloseDocument("tab-review-a.txt")
                    closeDialog.customFooterActions[1].trigger()
                    check(controller.open_document_ids.indexOf("tab-review-a.txt") === -1, "Discard kept the tab open")
                    check(controller.active_document_id === "tab-review-renamed.txt", "Closing the active tab did not select its neighbor")
                    console.log("KDE document tabs/undo isolation/rename/save-all/cancel/discard smoke test passed")
                    SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
                }
            } catch (error) {
                console.error(error)
                SmokeLifecycle.quit(controller, ApplicationLifecycle, 1)
            }
        }
    }
}
