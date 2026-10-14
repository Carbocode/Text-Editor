import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Basic as Basic
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.qmlmodels
import org.nativeworkspace.Syntax 1.0
import org.nativeworkspace.Editor 1.0

Item {
    id: root
    property var controller
    property bool enableBrowse: false
    property bool showingHome: true
    property alias homeWindow: homeWindow
    property bool creatingDirectory: false
    property int sidebarTab: 0 // 0: files, 1: text search, 2-5: empty tabs
    property string fileNameQuery: ""
    onSidebarTabChanged: scheduleSidebarUpdate()
    onFileNameQueryChanged: invalidateSidebar(0)
    property bool syncingEditor: false
    property bool updatingHighlight: false
    property var minimapLines: [""]
    property var minimapOffsets: [0]
    property var minimapKinds: []
    property var minimapGlyphs: []
    property string minimapSource: ""
    property var minimapSpans: []
    property int selectedResultIndex: -1
    property string selectedDirectoryPath: ""
    property alias editorText: editor.text
    property alias editorFont: editor.font
    property alias editorSelectedText: editor.selectedText
    property real editorScrollY: editorScroll.contentItem.contentY
    property int searchFileCount: textSearchFileIds().length
    property var collapsedDirectories: ({})
    property var orderedSidebarEntries: []
    property var visibleSidebarEntries: []
    // Keep Files and Search data independently; tab changes only select a cache.
    property var sidebarCaches: [null, null]
    property var searchSidebarEntries: []
    property bool fileTreeNeedsRestore: false
    property var childEntriesByParent: ({})
    property var treeIndexPaths: ({})
    property bool restoringFileTree: false
    property var matchIndicesByFile: ({})
    property bool sidebarEntriesDirty: true
    property string lastWorkspaceRoot: ""
    property string renamePath: ""
    property string renameName: ""
    property var activeRenameEditor: null
    property string draggingPath: ""
    property string hoverDirectory: ""
    property bool hoveringDrop: false
    property bool validDrop: false
    property var temporaryDragDirectories: []
    property bool dropCommitted: false
    property string committedDropDirectory: ""
    property bool finishingEntryDrag: false
    property bool dragPreviewVisible: false

    signal browseRequested()

    MouseArea {
        id: renameOutsideClick
        objectName: "renameOutsideClick"
        anchors.fill: parent
        z: 1000
        enabled: root.renamePath.length > 0 && root.activeRenameEditor && root.activeRenameEditor.visible
        acceptedButtons: Qt.AllButtons
        onPressed: function(mouse) {
            mouse.accepted = root.cancelRenameForClick(renameOutsideClick, mouse.x, mouse.y)
        }
    }

    SystemPalette { id: sidebarPalette }
    ListModel { id: sidebarModel }
    TreeModel {
        id: fileTreeModel
        TableModelColumn { display: "name" }
    }

    Timer {
        id: sidebarUpdate
        interval: 0
        onTriggered: if (root.sidebarEntriesDirty) root.rebuildOrderedEntries()
    }

    Timer {
        id: minimapUpdate
        interval: 180
        onTriggered: root.rebuildMinimap()
    }

    Connections {
        target: root.controller
        function onWorkspace_rootChanged() {
            if (root.lastWorkspaceRoot !== root.controller.workspace_root) {
                root.lastWorkspaceRoot = root.controller.workspace_root
                root.collapsedDirectories = ({})
                root.selectedDirectoryPath = ""
            }
            if (root.controller.workspace_root.length > 0) root.showingHome = false
            root.invalidateSidebar(0)
            root.invalidateSidebar(1)
        }
        function onDirectory_idsChanged() { root.invalidateSidebar(0) }
        function onDocument_idsChanged() { root.invalidateSidebar(0) }
        function onSearch_result_idsChanged() { root.invalidateSidebar(1) }
        function onSearch_result_linesChanged() { root.invalidateSidebar(1) }
        function onSearch_queryChanged() { root.invalidateSidebar(1) }
    }

    Component.onCompleted: {
        scheduleSidebarUpdate()
        var encoded = root.controller ? root.controller.minimap_glyphs() : ""
        var glyphs = []
        for (var glyph = 0; glyph < encoded.length; glyph += 2)
            glyphs.push(parseInt(encoded.slice(glyph, glyph + 2), 16))
        minimapGlyphs = glyphs
        updateHighlight()
    }

    property string highlightedFileName: ""
    function updateHighlight() {
        if (updatingHighlight || !root.controller) return
        var source = editor.text
        var fileName = root.controller.active_document_id.split("/").pop()
        if (source === minimapSource && fileName === highlightedFileName) return
        updatingHighlight = true
        highlightedFileName = fileName
        var spans = root.controller.highlight_document(source)
        syntaxHighlighter.spans = spans
        minimapSource = source
        minimapSpans = spans
        minimapUpdate.restart()
        updatingHighlight = false
    }

    function rebuildMinimap() {
        var source = minimapSource
        var spans = minimapSpans
        var kinds = []
        for (var index = 0; index < spans.length; ++index) {
            var parts = spans[index].split(":")
            var end = Math.min(source.length, Number(parts[1]))
            for (var position = Number(parts[0]); position < end; ++position)
                kinds[position] = Number(parts[2])
        }
        minimapKinds = kinds
        var lines = source.split("\n")
        var offsets = []
        var nextOffset = 0
        for (var line = 0; line < lines.length; ++line) {
            offsets.push(nextOffset)
            nextOffset += lines[line].length + 1
        }
        minimapOffsets = offsets
        minimapLines = lines
        minimap.requestPaint()
    }

    function invalidateSidebar(tab) {
        sidebarCaches[tab] = null
        if (sidebarTab === tab) scheduleSidebarUpdate()
    }

    function cacheCurrentSidebar() {
        if (sidebarTab >= 2) return
        sidebarCaches[sidebarTab] = {
            ordered: orderedSidebarEntries,
            visible: visibleSidebarEntries,
            children: childEntriesByParent,
            matches: matchIndicesByFile
        }
    }

    function scheduleSidebarUpdate() {
        sidebarEntriesDirty = true
        sidebarUpdate.restart()
    }

    function flushEditor() {
        if (controller.active_document_id.length > 0 && editor.text !== controller.active_document_text) {
            controller.change_document(editor.text)
            return controller.error_message.length === 0
        }
        return true
    }

    function saveActive() {
        if (flushEditor()) controller.save_document()
    }

    function openWorkspace(path) {
        if (!flushEditor()) return
        controller.open_workspace(path)
        if (controller.error_message.length === 0) showingHome = false
    }

    function createEntry(path, directory) {
        controller.create_entry(path, directory)
        if (!directory && controller.error_message.length === 0)
            controller.open_document(path)
    }

    function beginRename(path) {
        activeRenameEditor = null
        renamePath = path
        renameName = path.split("/").pop()
        Qt.callLater(root.focusInlineRename)
    }

    function cancelRenameForClick(item, x, y) {
        if (!activeRenameEditor || renamePath.length === 0) return false
        var point = activeRenameEditor.mapFromItem(item, x, y)
        if (activeRenameEditor.contains(point)) return false
        renamePath = ""
        activeRenameEditor = null
        return true
    }

    function clickSidebarEntry(path, directory) {
        if (renamePath.length > 0 || draggingPath.length > 0) return
        if (directory) {
            selectedDirectoryPath = path
            toggleDirectory(path)
            return
        }
        var selected = selectedDirectoryPath.length === 0 && selectedResultIndex < 0 && controller.active_document_id === path
        if (selected) { beginRename(path); return }
        if (flushEditor()) {
            selectedDirectoryPath = ""
            controller.open_document(path)
            selectedResultIndex = -1
        }
    }

    function focusInlineRename() {
        if (sidebarTab === 1) {
            if (!activeRenameEditor || !activeRenameEditor.visible) return
            if (activeRenameEditor.activeFocus) return
        } else {
            if (sidebarEntriesDirty) rebuildOrderedEntries()
            fileTree.forceLayout()
            var row = treeRowForPath(renamePath)
            if (row < 0) return
            var delegate = fileTree.itemAtCell(Qt.point(0, row))
            if (!delegate || !delegate.renameEditor) return
            if (activeRenameEditor === delegate.renameEditor && activeRenameEditor.activeFocus) return
            activeRenameEditor = delegate.renameEditor
        }
        activeRenameEditor.text = renameName
        activeRenameEditor.forceActiveFocus(Qt.OtherFocusReason)
        var dot = controller.directory_ids.indexOf(renamePath) === -1 ? renameName.lastIndexOf(".") : -1
        activeRenameEditor.select(0, dot > 0 ? dot : renameName.length)
    }

    function commitRename() {
        if (renamePath.length === 0 || !flushEditor()) return
        controller.rename_entry(renamePath, renameName)
        if (controller.error_message.length === 0) renamePath = ""
    }

    function parentDirectory(path) { return path.split("/").slice(0, -1).join("/") }

    function isPendingMove(path) {
        var target = controller.pending_move_target
        return target && (path === target || path.startsWith(target + "/"))
    }

    function canDrop(source, directory) {
        if (controller.pending_move_target.length > 0) return false
        if (source.length === 0) return false
        var target = (directory.length > 0 ? directory + "/" : "") + source.split("/").pop()
        return target !== source && !target.startsWith(source + "/")
            && controller.directory_ids.indexOf(target) === -1
            && !controller.document_ids.some(path => path === target || path.startsWith(target + "/"))
    }

    function restoreDragDirectories(destination) {
        var keep = []
        var closing = []
        for (var folder of temporaryDragDirectories) {
            if (destination !== null && (destination === folder || destination.startsWith(folder + "/"))) keep.push(folder)
            else closing.push(folder)
        }
        temporaryDragDirectories = keep
        closing.sort((a, b) => b.length - a.length)
        for (var path of closing) if (collapsedDirectories[path] === false) {
            if (sidebarTab === 0 && fileNameQuery.trim().length === 0 && treeRowForPath(path) >= 0) {
                toggleDirectory(path)
            } else {
                var restored = Object.assign({}, collapsedDirectories)
                restored[path] = true
                collapsedDirectories = restored
                invalidateSidebar(0)
            }
        }
    }

    function hoverDrop(directory) {
        if (draggingPath.length === 0) return
        leaveDropFolder.stop()
        if (hoveringDrop && hoverDirectory === directory) {
            validDrop = canDrop(draggingPath, directory)
            return
        }
        expandDropFolder.stop()
        restoreDragDirectories(directory)
        hoverDirectory = directory
        hoveringDrop = true
        validDrop = canDrop(draggingPath, directory)
        if (validDrop && directory.length > 0 && collapsedDirectories[directory] !== false)
            expandDropFolder.restart()
    }

    function leaveDrop(directory) {
        if (hoveringDrop && hoverDirectory === directory) {
            leaveDropFolder.directory = directory
            leaveDropFolder.restart()
        }
    }

    function dropEntry(directory) {
        beginDropTrace()
        if (!canDrop(draggingPath, directory)) {
            finishEntryDrag(true)
            controller.drop_handler_finished(false)
            return false
        }
        var source = draggingPath
        var temporaryFolders = temporaryDragDirectories.slice()
        dragPreviewVisible = false
        controller.drop_preview_hidden()
        hoveringDrop = false
        validDrop = false
        // End hit testing and hover feedback before replacing tree rows.
        dropCommitted = true
        committedDropDirectory = directory
        finishEntryDrag(true)
        var dispatchStarted = Date.now()
        controller.move_entry(source, directory, editor.text)
        var dispatchMs = Date.now() - dispatchStarted
        selectedResultIndex = -1
        var succeeded = controller.error_message.length === 0
        if (succeeded && directory.length > 0) {
            var opened = Object.assign({}, collapsedDirectories)
            opened[directory] = false
            collapsedDirectories = opened
        }
        if (!succeeded) {
            temporaryDragDirectories = temporaryFolders
            restoreDragDirectories(null)
        }
        sidebarEntriesDirty = true
        sidebarUpdate.stop()
        var rebuildStarted = Date.now()
        rebuildOrderedEntries()
        if (sidebarTab === 0) {
            fileTree.forceLayout()
            restoreFileTreeExpansion()
        }
        if (Qt.application.arguments.indexOf("--trace-drop") !== -1)
            console.log("Drop dispatch=" + dispatchMs + "ms tree=" + (Date.now() - rebuildStarted) + "ms")
        if (succeeded) controller.drop_sidebar_ready()
        controller.drop_handler_finished(succeeded)
        return succeeded
    }

    property bool dropTraceStarted: false
    Timer {
        interval: 50
        repeat: true
        running: root.controller.devtools_available
        onTriggered: root.controller.diagnostic_ui_heartbeat()
    }
    function beginDropTrace() {
        if (!dropTraceStarted) { controller.begin_sidebar_drop(); dropTraceStarted = true }
    }

    function startEntryDrag(path, point) {
        finishEntryDrag(true)
        dropTraceStarted = false
        dropCommitted = false
        renamePath = ""
        dragGhost.x = point.x + 12
        dragGhost.y = point.y + 12
        draggingPath = path
        dragPreviewVisible = true
    }

    function finishEntryDrag(cancelled) {
        if (finishingEntryDrag) return
        finishingEntryDrag = true
        var releasing = !cancelled && draggingPath.length > 0 && !dropCommitted
        if (releasing) beginDropTrace()
        dragPreviewVisible = false
        if (releasing) {
            controller.drop_preview_hidden()
            if (dragGhost.Drag.drop() === Qt.IgnoreAction) controller.drop_handler_finished(false)
        }
        draggingPath = ""
        hoveringDrop = false
        hoverDirectory = ""
        validDrop = false
        expandDropFolder.stop()
        leaveDropFolder.stop()
        restoreDragDirectories(dropCommitted ? committedDropDirectory : null)
        temporaryDragDirectories = []
        dropCommitted = false
        finishingEntryDrag = false
    }

    Timer {
        id: leaveDropFolder
        property string directory: ""
        interval: 50
        onTriggered: {
            if (root.hoveringDrop && root.hoverDirectory === directory) {
                expandDropFolder.stop()
                root.hoveringDrop = false
            }
        }
    }

    Timer {
        id: expandDropFolder
        interval: 450
        onTriggered: {
            if (root.hoveringDrop && root.canDrop(root.draggingPath, root.hoverDirectory)
                && root.collapsedDirectories[root.hoverDirectory] !== false) {
                root.temporaryDragDirectories = root.temporaryDragDirectories.concat([root.hoverDirectory])
                root.toggleDirectory(root.hoverDirectory)
            }
        }
    }

    Shortcut {
        sequence: "Escape"
        enabled: root.draggingPath.length > 0 || root.renamePath.length > 0
        onActivated: { root.renamePath = ""; root.finishEntryDrag(true) }
    }

    Rectangle {
        id: dragGhost
        objectName: "sidebarDragGhost"
        z: 1000
        visible: root.dragPreviewVisible
        width: ghostLabel.implicitWidth + 36
        height: 34
        radius: 5
        color: sidebarPalette.base
        opacity: 0.9
        border.color: sidebarPalette.highlight
        border.width: 1
        Drag.active: root.draggingPath.length > 0
        // Qt excludes the source item's subtree from drop hit testing. Using
        // the workspace pane here would exclude every destination row.
        Drag.source: dragGhost
        Drag.keys: ["native-workspace-entry"]
        Drag.hotSpot.x: -12
        Drag.hotSpot.y: -12
        Drag.supportedActions: Qt.MoveAction
        Drag.proposedAction: Qt.MoveAction
        Rectangle {
            anchors.fill: parent
            anchors.topMargin: 4
            anchors.leftMargin: 3
            anchors.rightMargin: -3
            anchors.bottomMargin: -4
            radius: 5
            color: "#40000000"
            z: -1
        }
        Label {
            id: ghostLabel
            anchors.centerIn: parent
            text: root.draggingPath.split("/").pop()
        }
    }

    Dialog {
        id: createEntryDialog
        anchors.centerIn: parent
        modal: true
        title: root.creatingDirectory ? "New Folder" : "New File"
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: TextField {
            id: newEntryPath
            implicitWidth: 320
            placeholderText: "Path relative to workspace"
            onAccepted: createEntryDialog.accept()
        }
        onOpened: {
            newEntryPath.text = ""
            newEntryPath.forceActiveFocus()
        }
        onAccepted: root.createEntry(newEntryPath.text.trim(), root.creatingDirectory)
    }

    Shortcut {
        sequences: [StandardKey.Save]
        enabled: root.controller && root.controller.active_document_dirty
            && !root.controller.active_document_external_change
        onActivated: root.saveActive()
    }

    function openSearchResult(index, id) {
        selectedDirectoryPath = ""
        if (!flushEditor()) return
        var start = controller.search_result_starts[index]
        var end = controller.search_result_ends[index]
        controller.open_document(id)
        if (controller.active_document_id !== id) return
        selectedResultIndex = index
        if (start !== "") {
            Qt.callLater(function() {
                var position = Number(start)
                editor.select(position, Number(end))
                editor.forceActiveFocus()
                var flick = editorScroll.contentItem
                var rect = editor.positionToRectangle(position)
                if (rect.y < flick.contentY)
                    flick.contentY = rect.y
                else if (rect.y + rect.height > flick.contentY + flick.height)
                    flick.contentY = rect.y + rect.height - flick.height
                if (rect.x < flick.contentX)
                    flick.contentX = rect.x
                else if (rect.x + rect.width > flick.contentX + flick.width)
                    flick.contentX = rect.x + rect.width - flick.width
            })
        }
    }

    function textSearchFileIds() {
        var ids = controller.search_result_ids
        var lines = controller.search_result_lines
        var files = []
        var seen = ({})
        for (var i = 0; i < ids.length; ++i) {
            if (lines[i] !== "" && !seen["#" + ids[i]]) {
                files.push(ids[i])
                seen["#" + ids[i]] = true
            }
        }
        return files
    }

    function fileIds() {
        if (sidebarTab >= 2) return []
        if (sidebarTab === 1) return textSearchFileIds()
        var ids = controller.document_ids
        var files = []
        var seen = ({})
        var needle = fileNameQuery.trim().toLowerCase()
        for (var i = 0; i < ids.length; ++i) {
            if (sidebarTab === 0 && needle.length > 0 && !ids[i].split("/").pop().toLowerCase().includes(needle)) continue
            if (!seen["#" + ids[i]]) {
                files.push(ids[i])
                seen["#" + ids[i]] = true
            }
        }
        return files
    }

    function documentThemeIcon(path) {
        var name = path.split("/").pop().toLowerCase()
        if (name.endsWith(".md")) return "text-markdown"
        if (name.endsWith(".json")) return "application-json"
        if (name.endsWith(".xml")) return "application-xml"
        if (name.endsWith(".html") || name.endsWith(".htm")) return "text-html"
        if (name.endsWith(".py")) return "text-x-python"
        if (name.endsWith(".cpp") || name.endsWith(".cc") || name.endsWith(".cxx")) return "text-x-c++src"
        if (name.endsWith(".h") || name.endsWith(".hpp")) return "text-x-c++hdr"
        if (name.endsWith(".js") || name.endsWith(".ts")) return "application-javascript"
        return "text-x-generic"
    }

    function rebuildOrderedEntries() {
        sidebarEntriesDirty = false
        sidebarUpdate.stop()
        if (sidebarTab >= 2) {
            visibleSidebarEntries = []
            return
        }
        var cached = sidebarCaches[sidebarTab]
        if (cached) {
            orderedSidebarEntries = cached.ordered
            visibleSidebarEntries = cached.visible
            childEntriesByParent = cached.children
            if (sidebarTab === 1) matchIndicesByFile = cached.matches
            if (sidebarTab === 0 && fileTreeNeedsRestore)
                Qt.callLater(root.restoreFileTreeExpansion)
            return
        }
        var files = fileIds()
        var matches = ({})
        if (sidebarTab === 1 && root.controller.search_query.length > 0) {
            var resultIds = root.controller.search_result_ids
            var resultLines = root.controller.search_result_lines
            for (var result = 0; result < resultIds.length; ++result) {
                if (resultLines[result] === "") continue
                var resultKey = "#" + resultIds[result]
                if (!matches[resultKey]) matches[resultKey] = []
                matches[resultKey].push(result)
            }
        }
        if (sidebarTab === 1) matchIndicesByFile = matches
        var directories = []
        var workspaceDirectories = root.controller.directory_ids
        var seenDirectories = ({})
        if (sidebarTab === 0 && fileNameQuery.trim().length === 0) {
            for (var k = 0; k < workspaceDirectories.length; ++k) {
                var directory = workspaceDirectories[k]
                directories.push(directory)
                seenDirectories["#" + directory] = true
            }
        }
        if (sidebarTab === 0 && fileNameQuery.trim().length > 0) {
            var folderNeedle = fileNameQuery.trim().toLowerCase()
            for (var k = 0; k < workspaceDirectories.length; ++k) {
                var namedFolder = workspaceDirectories[k]
                if (namedFolder.split("/").pop().toLowerCase().includes(folderNeedle)) {
                    directories.push(namedFolder)
                    seenDirectories["#" + namedFolder] = true
                }
            }
        }
        if (sidebarTab === 1 || fileNameQuery.trim().length > 0) {
            for (var i = 0; i < files.length; ++i) {
                var parts = files[i].split("/")
                for (var j = 1; j < parts.length; ++j) {
                    var parent = parts.slice(0, j).join("/")
                    if (!seenDirectories["#" + parent]) {
                        directories.push(parent)
                        seenDirectories["#" + parent] = true
                    }
                }
            }
            var matchedDirectories = directories.slice()
            for (var d = 0; d < matchedDirectories.length; ++d) {
                var folderParts = matchedDirectories[d].split("/")
                for (var ancestor = 1; ancestor < folderParts.length; ++ancestor) {
                    var folderParent = folderParts.slice(0, ancestor).join("/")
                    if (!seenDirectories["#" + folderParent]) {
                        directories.push(folderParent)
                        seenDirectories["#" + folderParent] = true
                    }
                }
            }
            directories.sort()
            files.sort()
        }
        var foldersByParent = ({})
        var filesByParent = ({})
        function addChild(table, path) {
            var separator = path.lastIndexOf("/")
            var parent = separator < 0 ? "" : path.substring(0, separator)
            var key = "#" + parent
            if (!table[key]) table[key] = []
            table[key].push(path)
        }
        for (var d = 0; d < directories.length; ++d)
            addChild(foldersByParent, directories[d])
        for (var f = 0; f < files.length; ++f)
            addChild(filesByParent, files[f])
        var childEntries = ({})
        for (var folderParent in foldersByParent) {
            childEntries[folderParent] = foldersByParent[folderParent].map(function(path) {
                return { path: path, directory: true }
            })
        }
        for (var fileParent in filesByParent) {
            if (!childEntries[fileParent]) childEntries[fileParent] = []
            for (var child of filesByParent[fileParent])
                childEntries[fileParent].push({ path: child, directory: false })
        }
        childEntriesByParent = childEntries
        if (sidebarTab === 0) {
            var indexPaths = ({})
            function treeRows(parent, parentIndex) {
                var entries = childEntries["#" + parent] || []
                return entries.map(function(entry, row) {
                    var indexPath = parentIndex.concat([row])
                    indexPaths["#" + entry.path] = indexPath
                    var node = {
                        name: entry.path.split("/").pop(),
                        path: entry.path,
                        directory: entry.directory
                    }
                    if (entry.directory) node.rows = treeRows(entry.path, indexPath)
                    return node
                })
            }
            restoringFileTree = true
            fileTreeNeedsRestore = true
            var rootRows = treeRows("", [])
            treeIndexPaths = indexPaths
            fileTreeModel.rows = rootRows
            Qt.callLater(root.restoreFileTreeExpansion)
        }
        var paths = []
        function appendChildren(parent) {
            for (var entry of childEntries["#" + parent] || []) {
                paths.push(entry)
                if (entry.directory) appendChildren(entry.path)
            }
        }
        appendChildren("")
        orderedSidebarEntries = paths
        updateVisibleEntries()
        cacheCurrentSidebar()
    }

    function updateVisibleEntries() {
        var searching = sidebarTab === 1 || fileNameQuery.trim().length > 0
        var visible = []
        var hiddenPrefix = ""
        for (var i = 0; i < orderedSidebarEntries.length; ++i) {
            var entry = orderedSidebarEntries[i]
            if (hiddenPrefix !== "") {
                if (entry.path.startsWith(hiddenPrefix)) continue
                hiddenPrefix = ""
            }
            visible.push(entry)
            if (!searching && entry.directory && collapsedDirectories[entry.path] !== false)
                hiddenPrefix = entry.path + "/"
        }
        // Only Search uses the ListModel. Keep it populated while Files or an
        // empty panel is selected, rather than removing and reinserting rows.
        if (sidebarTab !== 1) {
            visibleSidebarEntries = visible
            return
        }
        var previous = searchSidebarEntries
        var start = 0
        while (start < previous.length && start < visible.length
               && previous[start].path === visible[start].path
               && previous[start].directory === visible[start].directory)
            ++start
        var oldEnd = previous.length
        var newEnd = visible.length
        while (oldEnd > start && newEnd > start
               && previous[oldEnd - 1].path === visible[newEnd - 1].path
               && previous[oldEnd - 1].directory === visible[newEnd - 1].directory) {
            --oldEnd
            --newEnd
        }
        if (oldEnd > start)
            sidebarModel.remove(start, oldEnd - start)
        for (var row = start; row < newEnd; ++row)
            sidebarModel.insert(row, visible[row])
        searchSidebarEntries = visible
        visibleSidebarEntries = visible
    }

    function sidebarEntries() {
        if (sidebarEntriesDirty) rebuildOrderedEntries()
        return visibleSidebarEntries
    }

    function treeMatchesSidebarEntries() {
        for (var row = 0; row < visibleSidebarEntries.length; ++row) {
            var indexPath = treeIndexPaths["#" + visibleSidebarEntries[row].path]
            if (!indexPath || fileTree.rowAtIndex(fileTreeModel.index(indexPath, 0)) !== row)
                return false
        }
        return true
    }

    function treeRowForPath(path) {
        var indexPath = treeIndexPaths["#" + path]
        return indexPath ? fileTree.rowAtIndex(fileTreeModel.index(indexPath, 0)) : -1
    }

    function restoreFileTreeExpansion() {
        if (sidebarTab !== 0) {
            restoringFileTree = false
            return
        }
        if (sidebarTab === 0) {
            if (fileNameQuery.trim().length > 0) {
                fileTree.expandRecursively(-1)
            } else {
                function expandChildren(parent) {
                    for (var entry of childEntriesByParent["#" + parent] || []) {
                        if (!entry.directory || collapsedDirectories[entry.path] !== false) continue
                        var modelIndex = fileTreeModel.index(treeIndexPaths["#" + entry.path], 0)
                        var row = fileTree.rowAtIndex(modelIndex)
                        if (row >= 0 && !fileTree.isExpanded(row)) fileTree.expand(row)
                        expandChildren(entry.path)
                    }
                }
                expandChildren("")
            }
        }
        fileTreeNeedsRestore = false
        restoringFileTree = false
        if (renamePath.length > 0) Qt.callLater(root.focusInlineRename)
    }

    function toggleDirectory(path) {
        if (sidebarEntriesDirty) rebuildOrderedEntries()
        var visible = visibleSidebarEntries
        var index = visible.findIndex(entry => entry.directory && entry.path === path)
        if (index < 0) return
        var prefix = path + "/"
        var oldEnd = index + 1
        while (oldEnd < visible.length && visible[oldEnd].path.startsWith(prefix))
            ++oldEnd
        var next = ({})
        for (var key in collapsedDirectories) next[key] = collapsedDirectories[key]
        var expanding = collapsedDirectories[path] !== false
        next[path] = expanding ? false : true
        collapsedDirectories = next
        var children = []
        function appendVisibleChildren(parent) {
            for (var entry of childEntriesByParent["#" + parent] || []) {
                children.push(entry)
                if (entry.directory && collapsedDirectories[entry.path] === false)
                    appendVisibleChildren(entry.path)
            }
        }
        if (expanding) appendVisibleChildren(path)
        var oldCount = oldEnd - index - 1
        if (sidebarTab === 1) {
            if (oldCount > 0) sidebarModel.remove(index + 1, oldCount)
            for (var row = 0; row < children.length; ++row)
                sidebarModel.insert(index + 1 + row, children[row])
        }
        visibleSidebarEntries = visible.slice(0, index + 1).concat(children, visible.slice(oldEnd))
        if (sidebarTab === 1) {
            searchSidebarEntries = visibleSidebarEntries
            invalidateSidebar(0)
        }
        cacheCurrentSidebar()
        if (sidebarTab === 0 && fileNameQuery.trim().length === 0 && treeIndexPaths["#" + path]) {
            var treeRow = fileTree.rowAtIndex(fileTreeModel.index(treeIndexPaths["#" + path], 0))
            if (treeRow >= 0 && fileTree.isExpanded(treeRow) !== expanding) {
                if (expanding) fileTree.expand(treeRow)
                else fileTree.collapse(treeRow)
            }
            if (expanding) restoreFileTreeExpansion()
        }
    }

    function matchIndicesForFile(id) {
        return matchIndicesByFile["#" + id] || []
    }

    Timer {
        interval: 250
        repeat: true
        running: root.controller && root.controller.workspace_name.length > 0
        onTriggered: if (root.flushEditor()) {
            root.controller.poll_workspace_changes()
            root.controller.poll_search_results()
        }
    }

    ApplicationWindow {
        id: homeWindow
        transientParent: null
        title: "Native Workspace"
        width: 640
        height: 560
        minimumWidth: 480
        minimumHeight: 400
        visible: root.enableBrowse && root.showingHome
        // Keep native window-manager decorations, including on KDE.
        onClosing: Qt.quit()
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 28
            spacing: 18
            Label {
                text: "Native Workspace"
                font.pixelSize: 30
                font.bold: true
            }
            Label {
                text: "A place for your files and ideas."
                opacity: 0.7
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            RowLayout {
                Button {
                    text: "Open Folder…"
                    highlighted: true
                    onClicked: root.browseRequested()
                }
                Button {
                    text: "Resume Workspace"
                    visible: root.controller.workspace_root.length > 0
                    onClicked: root.showingHome = false
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: palette.mid }
            Label { text: "Recent Workspaces"; font.pixelSize: 18; font.bold: true }
            Label {
                text: "Open a folder. It will appear here for quick access."
                visible: root.controller.recent_workspace_paths.length === 0
                opacity: 0.7
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            ListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                spacing: 6
                model: root.controller.recent_workspace_paths
                ScrollBar.vertical: ScrollBar { }
                delegate: Basic.ItemDelegate {
                    required property int index
                    required property string modelData
                    width: ListView.view.width
                    height: 68
                    background: Rectangle {
                        radius: 8
                        color: parent.hovered ? palette.alternateBase : palette.base
                        border.color: palette.mid
                    }
                    ToolTip.visible: hovered
                    ToolTip.text: modelData
                    contentItem: RowLayout {
                        spacing: 12
                        Label { text: "▸"; opacity: 0.6 }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4
                            Label {
                                text: root.controller.recent_workspace_names[index]
                                font.bold: true
                                Layout.fillWidth: true
                                elide: Text.ElideMiddle
                            }
                            Label {
                                text: modelData
                                opacity: 0.65
                                Layout.fillWidth: true
                                elide: Text.ElideMiddle
                            }
                        }
                    }
                    onClicked: root.openWorkspace(modelData)
                }
            }
            Label {
                text: root.controller.error_message
                visible: text.length > 0
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            Button {
                text: "Developer Tools"
                visible: root.controller.devtools_available
                onClicked: root.controller.open_devtools()
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.controller.workspace_name.length > 0
        ToolBar {
            Layout.fillWidth: true
            contentItem: RowLayout {
                spacing: 8
                Item { Layout.fillWidth: true }
                ToolButton {
                    text: "Home"
                    icon.name: "go-home"
                    icon.width: 20
                    icon.height: 20
                    display: AbstractButton.TextBesideIcon
                    onClicked: if (root.flushEditor()) root.showingHome = true
                }
            }
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: palette.mid
        }
        SplitView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Pane {
                SplitView.preferredWidth: 240
                SplitView.minimumWidth: 180
                padding: 0
                background: Rectangle {
                    color: palette.window
                    border.color: palette.mid
                    border.width: 1
                }
                Item {
                    anchors.fill: parent
                    Column {
                        id: sidebarTabs
                        anchors.top: parent.top
                        anchors.left: parent.left
                        anchors.bottom: parent.bottom
                        width: 36
                        spacing: 0
                        Repeater {
                            model: [
                                { title: "Files", icon: "folder", source: "" },
                                { title: "Search", icon: "edit-find", source: "" },
                                { title: "Git", icon: "", source: "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/icons/git-branch.png" },
                                { title: "Bookmarks", icon: "", source: "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/icons/bookmark.png" },
                                { title: "Warnings", icon: "dialog-warning", source: "" },
                                { title: "Errors", icon: "dialog-error", source: "" }
                            ]
                            delegate: ToolButton {
                                id: tabButton
                                required property int index
                                required property var modelData
                                text: modelData.title
                                icon.name: modelData.icon
                                icon.source: modelData.source
                                icon.width: 16
                                icon.height: 16
                                display: AbstractButton.IconOnly
                                width: sidebarTabs.width
                                height: 80
                                leftPadding: 0
                                rightPadding: 0
                                topPadding: tabLabel.width + 4
                                bottomPadding: 0
                                ToolTip.visible: hovered
                                ToolTip.text: text
                                checkable: true
                                checked: root.sidebarTab === index
                                Label {
                                    id: tabLabel
                                    anchors.centerIn: parent
                                    anchors.verticalCenterOffset: -10
                                    width: Math.min(implicitWidth, 58)
                                    horizontalAlignment: Text.AlignHCenter
                                    elide: Text.ElideRight
                                    rotation: -90
                                    text: tabButton.text
                                    font.pixelSize: 10
                                }
                                onClicked: {
                                    root.sidebarTab = index
                                    root.selectedResultIndex = -1
                                }
                            }
                        }
                    }
                    Rectangle {
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        color: palette.base
                        border.color: palette.mid
                        border.width: 1
                    }
                    RowLayout {
                        id: textSearchRow
                        anchors.top: parent.top
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        anchors.margins: 6
                        visible: root.sidebarTab === 1
                        TextField {
                            Layout.fillWidth: true
                            text: root.controller.search_query
                            placeholderText: "Search in files"
                            onTextEdited: {
                                root.selectedResultIndex = -1
                                root.controller.search_files(text)
                            }
                        }
                        BusyIndicator {
                            running: root.controller.search_in_progress
                            visible: running
                            Layout.preferredWidth: 18
                            Layout.preferredHeight: 18
                        }
                    }
                    TreeView {
                        id: fileTree
                        objectName: "fileTree"
                        anchors.top: parent.top
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        anchors.bottom: workspaceDropTarget.top
                        anchors.bottomMargin: 6
                        visible: root.sidebarTab === 0
                        clip: true
                        model: fileTreeModel
                        columnWidthProvider: function() { return width }
                        rowHeightProvider: function() { return 28 }
                        onWidthChanged: forceLayout()
                        onExpanded: function(row) {
                            if (root.sidebarTab !== 0 || root.restoringFileTree || root.fileNameQuery.trim().length > 0) return
                            var entry = fileTreeModel.getRow(fileTree.index(row, 0))
                            if (entry && root.collapsedDirectories[entry.path] !== false)
                                root.toggleDirectory(entry.path)
                        }
                        onCollapsed: function(row) {
                            if (root.sidebarTab !== 0 || root.restoringFileTree || root.fileNameQuery.trim().length > 0) return
                            var entry = fileTreeModel.getRow(fileTree.index(row, 0))
                            if (entry && root.collapsedDirectories[entry.path] === false)
                                root.toggleDirectory(entry.path)
                        }
                        Label {
                            anchors.centerIn: parent
                            visible: root.visibleSidebarEntries.length === 0
                            text: root.fileNameQuery.length > 0 ? "No matching names" : "No files"
                        }
                        // Native macOS controls reject custom contentItem. This
                        // delegate supports our icon, rename field and overlays.
                        delegate: Basic.TreeViewDelegate {
                            id: treeRow
                            topPadding: 2
                            bottomPadding: 2
                            indicator.height: 28
                            property alias renameEditor: renameField
                            property bool pooled: false
                            TableView.onPooled: pooled = true
                            TableView.onReused: pooled = false
                            // Expanding changes the mapping of view rows even
                            // when a reused delegate keeps the same row number.
                            property var entry: row < fileTree.rows && model.display !== undefined
                                ? fileTreeModel.getRow(fileTree.index(row, 0)) : null
                            onEntryChanged: if (root.renamePath.length > 0) Qt.callLater(root.focusInlineRename)
                            property bool pendingMove: entry && root.isPendingMove(entry.path)
                            text: entry ? entry.name : ""
                            ToolTip.visible: hovered && pendingMove
                            ToolTip.text: "Moving… Waiting for confirmation"
                            // TreeViewDelegate styles can provide a text-only
                            // contentItem, so draw the bundled icon explicitly.
                            contentItem: RowLayout {
                                spacing: 4
                                Image {
                                    objectName: "fileTreeIcon"
                                    opacity: treeRow.pendingMove ? 0.5 : 1
                                    source: treeRow.entry ? (treeRow.entry.directory
                                        ? root.controller.folder_icon(treeRow.entry.path, treeRow.expanded)
                                        : root.controller.file_icon(treeRow.entry.path)) : ""
                                    sourceSize.width: 32
                                    sourceSize.height: 32
                                    fillMode: Image.PreserveAspectFit
                                    Layout.preferredWidth: 16
                                    Layout.preferredHeight: 16
                                    Layout.minimumWidth: 16
                                    Layout.maximumWidth: 16
                                    Layout.minimumHeight: 16
                                    Layout.maximumHeight: 16
                                    Layout.alignment: Qt.AlignVCenter
                                }
                                Label {
                                    objectName: "sidebarEntryLabel"
                                    opacity: treeRow.pendingMove ? 0.5 : 1
                                    verticalAlignment: Text.AlignVCenter
                                    Layout.fillHeight: true
                                    visible: !renameField.visible
                                    text: treeRow.text
                                    font: treeRow.font
                                    color: treeRow.highlighted ? treeRow.palette.highlightedText : treeRow.palette.text
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    Layout.alignment: Qt.AlignVCenter
                                }
                                BusyIndicator {
                                    objectName: "pendingMoveIndicator"
                                    visible: treeRow.pendingMove
                                    running: visible && !treeRow.pooled
                                    Layout.preferredWidth: 16
                                    Layout.preferredHeight: 16
                                    Accessible.name: "Moving… Waiting for confirmation"
                                }
                                Basic.TextField {
                                    id: renameField
                                    objectName: "sidebarRenameField"
                                    visible: !treeRow.pooled && treeRow.entry && root.renamePath === treeRow.entry.path
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    Layout.minimumHeight: 24
                                    Layout.preferredHeight: 24
                                    Layout.maximumHeight: 24
                                    Layout.alignment: Qt.AlignVCenter
                                    font: treeRow.font
                                    padding: 2
                                    leftPadding: 4
                                    rightPadding: 4
                                    clip: true
                                    text: visible ? root.renameName : ""
                                    onTextEdited: root.renameName = text
                                    onAccepted: root.commitRename()
                                    onActiveFocusChanged: if (!activeFocus && root.activeRenameEditor === renameField && visible)
                                        root.renamePath = ""
                                    Keys.onEscapePressed: root.renamePath = ""
                                    ToolTip.visible: activeFocus && root.controller.error_message.length > 0
                                    ToolTip.text: root.controller.error_message
                                }
                            }
                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                onTapped: if (treeRow.entry) {
                                    entryMenu.path = treeRow.entry.path
                                    entryMenu.popup()
                                }
                            }
                            Menu {
                                id: entryMenu
                                property string path: ""
                                MenuItem {
                                    text: "Rename…"
                                    onTriggered: root.beginRename(entryMenu.path)
                                }
                            }
                            Keys.onPressed: function(event) {
                                if (event.key === Qt.Key_F2 && entry) {
                                    root.beginRename(entry.path)
                                    event.accepted = true
                                }
                            }
                            DragHandler {
                                id: rowDrag
                                target: null
                                enabled: treeRow.entry && !treeRow.pendingMove && root.renamePath.length === 0
                                acceptedButtons: Qt.LeftButton
                                cursorShape: root.hoveringDrop && !root.validDrop ? Qt.ForbiddenCursor : Qt.ClosedHandCursor
                                onActiveChanged: {
                                    if (active) root.startEntryDrag(treeRow.entry.path,
                                        root.mapFromItem(null, centroid.scenePosition.x, centroid.scenePosition.y))
                                    else root.finishEntryDrag(false)
                                }
                                onCentroidChanged: if (active) {
                                    var point = root.mapFromItem(null, centroid.scenePosition.x, centroid.scenePosition.y)
                                    dragGhost.x = point.x + 12
                                    dragGhost.y = point.y + 12
                                }
                            }
                            DropArea {
                                anchors.fill: parent
                                keys: ["native-workspace-entry"]
                                property string directory: treeRow.entry ? (treeRow.entry.directory
                                    ? treeRow.entry.path : root.parentDirectory(treeRow.entry.path)) : ""
                                onEntered: function(drag) {
                                    if (drag.source !== dragGhost) { drag.accepted = false; return }
                                    root.hoverDrop(directory)
                                }
                                onPositionChanged: function(drag) {
                                    if (drag.source === dragGhost) root.hoverDrop(directory)
                                }
                                onExited: root.leaveDrop(directory)
                                onDropped: function(drop) {
                                    if (drop.source === dragGhost && root.dropEntry(directory)) drop.accept(Qt.MoveAction)
                                }
                            }
                            Rectangle {
                                anchors.fill: parent
                                z: 2
                                color: root.validDrop ? Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.14)
                                    : "#18d64545"
                                border.width: 2
                                border.color: root.validDrop ? palette.highlight : "#d64545"
                                visible: root.draggingPath.length > 0 && root.hoveringDrop && treeRow.entry && treeRow.entry.directory
                                    && root.hoverDirectory === treeRow.entry.path
                            }
                            rightPadding: 38
                            highlighted: entry && (entry.directory ? root.selectedDirectoryPath === entry.path
                                : root.selectedDirectoryPath.length === 0 && root.selectedResultIndex < 0 && root.controller.active_document_id === entry.path)
                            onClicked: if (entry) root.clickSidebarEntry(entry.path, entry.directory)
                            Row {
                                anchors.right: parent.right
                                anchors.rightMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 4
                                Label {
                                    text: "⚠"
                                    visible: treeRow.entry && !treeRow.entry.directory
                                        && root.controller.conflicted_document_ids.indexOf(treeRow.entry.path) !== -1
                                    color: palette.highlight
                                }
                                Label {
                                    text: "●"
                                    visible: treeRow.entry && !treeRow.entry.directory
                                        && root.controller.dirty_document_ids.indexOf(treeRow.entry.path) !== -1
                                }
                            }
                        }
                    }
                    Rectangle {
                        id: workspaceDropTarget
                        objectName: "workspaceDropTarget"
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        anchors.bottom: searchRow.top
                        anchors.bottomMargin: 6
                        height: visible ? 32 : 0
                        z: 5
                        visible: root.sidebarTab === 0 && root.draggingPath.length > 0
                        color: root.hoveringDrop && root.hoverDirectory === ""
                            ? Qt.rgba(sidebarPalette.highlight.r, sidebarPalette.highlight.g, sidebarPalette.highlight.b, 0.14)
                            : sidebarPalette.base
                        border.width: root.hoveringDrop && root.hoverDirectory === "" ? 2 : 1
                        border.color: root.hoveringDrop && root.hoverDirectory === "" && !root.validDrop
                            ? "#d64545" : sidebarPalette.highlight
                        Label {
                            anchors.centerIn: parent
                            text: "Move to workspace root"
                        }
                        DropArea {
                            anchors.fill: parent
                            keys: ["native-workspace-entry"]
                            onEntered: function(drag) {
                                if (drag.source === dragGhost) root.hoverDrop("")
                                else drag.accepted = false
                            }
                            onExited: root.leaveDrop("")
                            onDropped: function(drop) {
                                if (drop.source === dragGhost && root.dropEntry("")) drop.accept(Qt.MoveAction)
                            }
                        }
                    }
                    ListView {
                        id: fileList
                        objectName: "searchResults"
                        anchors.top: textSearchRow.bottom
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        visible: root.sidebarTab === 1
                        clip: true
                        model: sidebarModel
                        reuseItems: true
                        Label {
                            anchors.centerIn: parent
                            text: root.sidebarTab === 1 ? "No text matches" : (root.fileNameQuery.length > 0 ? "No matching names" : "No files")
                            visible: root.sidebarTab < 2 && root.controller.workspace_name.length > 0 && root.visibleSidebarEntries.length === 0 && !root.controller.search_in_progress
                        }
                        delegate: Column {
                            id: fileGroup
                            width: ListView.view.width
                            property string fileId: model.path
                            property bool isDirectory: model.directory
                            property int depth: fileId.split("/").length - 1
                            ItemDelegate {
                                objectName: "sidebarSearchFile:" + fileGroup.fileId
                                width: parent.width
                                height: 28
                                text: root.renamePath === fileGroup.fileId ? "" : fileGroup.fileId.split("/").pop()
                                icon.name: fileGroup.isDirectory
                                    ? (root.sidebarTab === 1 || root.fileNameQuery.length > 0 || root.collapsedDirectories[fileGroup.fileId] === false ? "folder-open" : "folder")
                                    : root.documentThemeIcon(fileGroup.fileId)
                                icon.source: fileGroup.isDirectory
                                    ? root.controller.folder_icon(fileGroup.fileId, root.sidebarTab === 1 || root.fileNameQuery.length > 0 || root.collapsedDirectories[fileGroup.fileId] === false)
                                    : root.controller.file_icon(fileGroup.fileId)
                                icon.color: "transparent"
                                icon.width: 16
                                icon.height: 16
                                leftPadding: 28 + fileGroup.depth * 16
                                rightPadding: 38
                                highlighted: fileGroup.isDirectory ? root.selectedDirectoryPath === fileGroup.fileId
                                    : root.selectedDirectoryPath.length === 0 && root.selectedResultIndex < 0 && root.controller.active_document_id === fileGroup.fileId
                                onClicked: root.clickSidebarEntry(fileGroup.fileId, fileGroup.isDirectory)
                                TapHandler {
                                    acceptedButtons: Qt.RightButton
                                    onTapped: searchEntryMenu.popup()
                                }
                                Menu {
                                    id: searchEntryMenu
                                    MenuItem {
                                        text: "Rename…"
                                        onTriggered: root.beginRename(fileGroup.fileId)
                                    }
                                }
                                Basic.TextField {
                                    id: searchRenameField
                                    objectName: "sidebarRenameField"
                                    visible: root.renamePath === fileGroup.fileId
                                    anchors.left: parent.left
                                    anchors.leftMargin: parent.leftPadding + parent.icon.width + parent.spacing
                                    anchors.right: parent.right
                                    anchors.rightMargin: parent.rightPadding
                                    anchors.verticalCenter: parent.verticalCenter
                                    height: 24
                                    text: visible ? root.renameName : ""
                                    onVisibleChanged: if (visible) {
                                        root.activeRenameEditor = searchRenameField
                                        Qt.callLater(root.focusInlineRename)
                                    }
                                    onTextEdited: root.renameName = text
                                    onAccepted: root.commitRename()
                                    onActiveFocusChanged: if (!activeFocus && root.activeRenameEditor === searchRenameField && visible)
                                        root.renamePath = ""
                                    Keys.onEscapePressed: root.renamePath = ""
                                }
                                Label {
                                    x: 10 + fileGroup.depth * 16
                                    anchors.verticalCenter: parent.verticalCenter
                                    visible: fileGroup.isDirectory
                                    text: root.sidebarTab === 0 && root.fileNameQuery.length === 0 && root.collapsedDirectories[fileGroup.fileId] !== false ? "▸" : "▾"
                                }
                                Row {
                                    anchors.right: parent.right
                                    anchors.rightMargin: 8
                                    anchors.verticalCenter: parent.verticalCenter
                                    spacing: 4
                                    Label {
                                        text: "⚠"
                                        visible: !fileGroup.isDirectory && root.controller.conflicted_document_ids.indexOf(fileGroup.fileId) !== -1
                                        color: palette.highlight
                                    }
                                    Label {
                                        text: "●"
                                        visible: !fileGroup.isDirectory && root.controller.dirty_document_ids.indexOf(fileGroup.fileId) !== -1
                                    }
                                }
                            }
                            Repeater {
                                model: fileGroup.isDirectory || root.controller.search_query.length === 0
                                    ? [] : root.matchIndicesForFile(fileGroup.fileId)
                                delegate: Basic.ItemDelegate {
                                    property int resultIndex: modelData
                                    width: fileGroup.width
                                    height: 23
                                    topPadding: 2
                                    bottomPadding: 2
                                    leftPadding: 32 + fileGroup.depth * 16
                                    highlighted: root.selectedResultIndex === resultIndex
                                    onClicked: root.openSearchResult(resultIndex, fileGroup.fileId)
                                    contentItem: Label {
                                        text: "↳ " + root.controller.search_result_lines[resultIndex] + ":" + root.controller.search_result_columns[resultIndex] + "  " + (root.controller.search_result_snippets[resultIndex] || "")
                                        font.pointSize: 9
                                        elide: Text.ElideRight
                                    }
                                }
                            }
                        }
                    }
                    RowLayout {
                        id: searchRow
                        visible: root.sidebarTab === 0
                        anchors.left: sidebarTabs.right
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.leftMargin: 6
                        anchors.rightMargin: 6
                        anchors.bottomMargin: 6
                        height: 26
                        spacing: 6
                        ToolButton {
                            text: "+"
                            font.pixelSize: 20
                            font.weight: Font.Medium
                            Layout.preferredWidth: 26
                            Layout.preferredHeight: 26
                            ToolTip.visible: hovered
                            ToolTip.text: "Add file or folder"
                            onClicked: addMenu.open()
                            Menu {
                                id: addMenu
                                MenuItem {
                                    text: "New File"
                                    onTriggered: {
                                        root.creatingDirectory = false
                                        createEntryDialog.open()
                                    }
                                }
                                MenuItem {
                                    text: "New Folder"
                                    onTriggered: {
                                        root.creatingDirectory = true
                                        createEntryDialog.open()
                                    }
                                }
                            }
                        }
                        TextField {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 26
                            font.pixelSize: 12
                            text: root.fileNameQuery
                            placeholderText: "Filter names"
                            onTextEdited: {
                                root.selectedResultIndex = -1
                                root.fileNameQuery = text
                            }
                        }
                    }
                }
            }
            ColumnLayout {
                SplitView.fillWidth: true
                SplitView.fillHeight: true
                spacing: 0
                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    color: palette.base
                    border.color: palette.mid
                    border.width: 1
                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 1
                        spacing: 0
                        FontMetrics { id: editorMetrics; font: editor.font }
                        FontInfo { id: resolvedEditorFont; font: editor.font }
                        Canvas {
                            id: lineNumbers
                            objectName: "editorLineNumbers"
                            Layout.preferredWidth: 48
                            Layout.fillHeight: true
                            property var lineStarts: {
                                var starts = [0]
                                var source = editor.text
                                for (var offset = source.indexOf("\n"); offset >= 0;
                                     offset = source.indexOf("\n", offset + 1))
                                    starts.push(offset + 1)
                                return starts
                            }
                            function numberBaseline(line) {
                                var rect = editor.positionToRectangle(lineStarts[line])
                                return rect.y - editorScroll.contentItem.contentY
                                    + (rect.height - editorMetrics.height) / 2 + editorMetrics.ascent
                            }
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.clearRect(0, 0, width, height)
                                if (editor.enabled) {
                                    ctx.fillStyle = editor.activeLineColor
                                    ctx.fillRect(0, editor.cursorRectangle.y - editorScroll.contentItem.contentY,
                                                 width, editor.cursorRectangle.height)
                                }
                                ctx.fillStyle = "#808080"
                                ctx.font = resolvedEditorFont.pixelSize + 'px "' + resolvedEditorFont.family + '"'
                                ctx.textAlign = "right"
                                var scrollY = editorScroll.contentItem.contentY
                                var position = editor.positionAt(editor.leftPadding, scrollY)
                                var low = 0
                                var high = lineStarts.length
                                while (low + 1 < high) {
                                    var middle = Math.floor((low + high) / 2)
                                    if (lineStarts[middle] <= position) low = middle
                                    else high = middle
                                }
                                for (var line = Math.max(0, low - 1); line < lineStarts.length; ++line) {
                                    var rect = editor.positionToRectangle(lineStarts[line])
                                    if (rect.y - scrollY >= height) break
                                    if (rect.y + rect.height - scrollY <= 0) continue
                                    ctx.fillText(String(line + 1), width - 8, numberBaseline(line))
                                }
                            }
                            MouseArea {
                                anchors.fill: parent
                                enabled: editor.enabled
                                onPressed: function(mouse) {
                                    editor.forceActiveFocus()
                                    editor.cursorPosition = editor.positionAt(editor.leftPadding,
                                        mouse.y + editorScroll.contentItem.contentY)
                                }
                            }
                        }
                        ScrollView {
                            id: editorScroll
                            objectName: "editorScroll"
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Basic.TextArea {
                                id: editor
                                objectName: "documentEditor"
                                width: Math.max(editorScroll.availableWidth, implicitWidth)
                                height: Math.max(editorScroll.availableHeight, implicitHeight)
                                property color activeLineColor: Qt.rgba(palette.highlight.r,
                                    palette.highlight.g, palette.highlight.b, 0.10)
                                background: Rectangle {
                                    color: "transparent"
                                    clip: true
                                    Rectangle {
                                        objectName: "editorActiveLine"
                                        // TextArea keeps its background fixed to the scroll viewport.
                                        y: editor.cursorRectangle.y - editorScroll.contentItem.contentY
                                        width: parent.width
                                        height: editor.cursorRectangle.height
                                        color: editor.activeLineColor
                                        visible: editor.enabled
                                    }
                                }
                                font: EditorFonts.fixedFont
                                textFormat: TextEdit.PlainText
                                wrapMode: TextEdit.NoWrap
                                EditorSyntaxHighlighter {
                                    id: syntaxHighlighter
                                    textDocument: editor.textDocument
                                }
                                enabled: root.controller.active_document_id.length > 0
                                placeholderText: "Select a document"
                                onCursorRectangleChanged: lineNumbers.requestPaint()
                                onActiveLineColorChanged: lineNumbers.requestPaint()
                                onEnabledChanged: lineNumbers.requestPaint()
                                onFontChanged: lineNumbers.requestPaint()
                                onTextChanged: {
                                    root.updateHighlight()
                                    lineNumbers.requestPaint()
                                    if (!root.syncingEditor && root.controller.active_document_id.length > 0 && text !== root.controller.active_document_text) {
                                        root.selectedResultIndex = -1
                                        root.controller.change_document(text)
                                    }
                                }
                            }
                        }
                        Canvas {
                            id: minimap
                            Layout.preferredWidth: 100
                            Layout.fillHeight: true
                            clip: true
                            property real contentHeight: root.minimapLines.length * 2
                            property real viewportTop: editorScroll.contentItem.contentY
                                / Math.max(1, editorScroll.contentItem.contentHeight) * contentHeight
                            property real viewportHeight: editorScroll.contentItem.height
                                / Math.max(1, editorScroll.contentItem.contentHeight) * contentHeight
                            property int mapOffset: Math.floor(Math.max(0,
                                Math.min(contentHeight - height, viewportTop - (height - viewportHeight) / 2)))
                            onMapOffsetChanged: requestPaint()
                            onHeightChanged: requestPaint()
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.clearRect(0, 0, width, height)
                                var lines = root.minimapLines
                                var palette = ["#9ba4ae", "#aa70d3", "#d88b44", "#5eb569", "#858e97", "#569ae0"]
                                var first = Math.max(0, Math.floor(mapOffset / 2))
                                var last = Math.min(lines.length, Math.ceil((mapOffset + height) / 2))
                                for (var line = first; line < last; ++line) {
                                    var offset = root.minimapOffsets[line]
                                    var y = line * 2 - mapOffset
                                    var x = 4
                                    var source = lines[line]
                                    for (var column = 0; column < source.length && x < width - 4; ++column) {
                                        var code = source.charCodeAt(column)
                                        if (code === 9) {
                                            x += 4 - (x - 4) % 4
                                        } else {
                                            if (code > 32) {
                                                ctx.fillStyle = palette[root.minimapKinds[offset + column] || 0]
                                                var glyphIndex = (code >= 32 && code <= 126 ? code - 32 : 95) * 2
                                                var top = root.minimapGlyphs[glyphIndex] || 0
                                                var bottom = root.minimapGlyphs[glyphIndex + 1] || 0
                                                if (top > 0) {
                                                    ctx.globalAlpha = Math.min(1, top / 128)
                                                    ctx.fillRect(x, y, 1, 1)
                                                }
                                                if (bottom > 0) {
                                                    ctx.globalAlpha = Math.min(1, bottom / 128)
                                                    ctx.fillRect(x, y + 1, 1, 1)
                                                }
                                                ctx.globalAlpha = 1
                                            }
                                            ++x
                                        }
                                    }
                                }
                            }
                            Rectangle {
                                width: minimap.width
                                y: minimap.viewportTop - minimap.mapOffset
                                height: Math.max(8, minimap.viewportHeight)
                                color: Qt.rgba(0.5, 0.5, 0.5, 0.2)
                            }
                            MouseArea {
                                anchors.fill: parent
                                property real dragOffset: 0
                                onPressed: {
                                    dragOffset = minimap.mapOffset
                                    moveTo(mouseY, dragOffset)
                                }
                                onPositionChanged: if (pressed) moveTo(mouseY, dragOffset)
                                onWheel: function(wheel) {
                                    var flick = editorScroll.contentItem
                                    flick.contentY = Math.max(0, Math.min(flick.contentHeight - flick.height,
                                        flick.contentY - wheel.angleDelta.y / 120 * editorMetrics.height * 3))
                                    wheel.accepted = true
                                }
                                function moveTo(y, offset) {
                                    var flick = editorScroll.contentItem
                                    flick.contentY = Math.max(0, Math.min(flick.contentHeight - flick.height,
                                        (y + offset) / Math.max(1, minimap.contentHeight)
                                            * flick.contentHeight - flick.height / 2))
                                }
                            }
                        }
                        Connections {
                            target: editorScroll.contentItem
                            function onContentYChanged() {
                                lineNumbers.requestPaint()
                            }
                        }
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 8
                    Layout.rightMargin: 8
                    visible: root.controller.active_document_external_change
                    Label {
                        text: "This file changed on disk"
                        Layout.fillWidth: true
                    }
                    Button {
                        text: "Reload (discard edits)"
                        onClicked: root.controller.reload_document()
                    }
                    Button {
                        text: "Overwrite disk"
                        onClicked: if (root.flushEditor()) root.controller.overwrite_document()
                    }
                }
            }
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: palette.mid
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 8
            Label { text: root.controller.active_document_id || "No document" }
            Label {
                text: root.controller.error_message || (root.controller.active_document_id.length === 0 ? "Ready" : (root.controller.active_document_dirty ? "Modified" : "Saved"))
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                elide: Text.ElideRight
            }
        }
    }

    Connections {
        target: root.controller
        function onActive_document_idChanged() { Qt.callLater(root.updateHighlight) }
        function onActive_document_textChanged() {
            if (editor.text !== root.controller.active_document_text) {
                root.selectedResultIndex = -1
                root.syncingEditor = true
                editor.text = root.controller.active_document_text
                root.syncingEditor = false
            }
        }
    }
}
