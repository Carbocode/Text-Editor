import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.qmlmodels
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Syntax 1.0
import org.nativeworkspace.Editor 1.0

Item {
    id: root
    SystemPalette { id: systemColors; colorGroup: SystemPalette.Active }
    // Keep selection accented when focus moves to the editor or another window.
    // The host's accent may differ from Breeze's default selection blue.
    Kirigami.Theme.highlightColor: systemColors.accent
    Kirigami.Theme.highlightedTextColor: systemColors.highlightedText
    property var controller
    property bool enableBrowse: false
    property bool showingHome: true
    property alias homeWindow: homeWindow
    property alias sidebar: workspaceSidebar
    property alias sidebarHeaderBar: sidebarTopBar
    property alias workspacePage: editorPage
    property bool creatingDirectory: false
    property int sidebarTab: 0 // Files, Search, Git, Bookmarks, Warnings, Errors
    readonly property var sidebarSections: [
        { title: "Files", icon: "folder" },
        { title: "Search", icon: "edit-find" },
        { title: "Git", icon: "vcs-branch", explanation: "Git integration is not available yet." },
        { title: "Bookmarks", icon: "bookmarks", explanation: "Bookmarks are not available yet." },
        { title: "Warnings", icon: "dialog-warning", explanation: "Workspace diagnostics are not available yet." },
        { title: "Errors", icon: "dialog-error", explanation: "Workspace diagnostics are not available yet." }
    ]
    // The typed name filter. The tree shows `fileFilter`, the query whose
    // matches the controller has published, so typing never filters inline.
    property string fileNameQuery: ""
    readonly property string fileFilter: controller ? controller.file_filter_query : ""
    onSidebarTabChanged: {
        scheduleSidebarUpdate()
        if (sidebarTab === 0) Qt.callLater(root.restoreFileTreeSelection)
    }
    onFileNameQueryChanged: if (controller) controller.filter_files(fileNameQuery)
    onFileFilterChanged: invalidateSidebar(0)
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
    property alias editorActiveLine: activeLine
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

    ListModel { id: sidebarModel }
    TreeModel {
        id: fileTreeModel
        TableModelColumn { display: "name"; edit: "name" }
        onDataChanged: function(topLeft, bottomRight, roles) {
            var entry = getRow(topLeft)
            if (!entry || entry.name === entry.path.split("/").pop()) return
            var source = entry.path
            var name = entry.name
            // Let the native edit delegate finish before updating the tree.
            Qt.callLater(function() {
                if (root.flushEditor()) root.controller.rename_entry(source, name)
                if (root.controller.error_message.length > 0) root.invalidateSidebar(0)
            })
        }
    }

    NativeTreeModel { id: nativeTreeModel; sourceModel: fileTreeModel }

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
        function onFile_filter_filesChanged() { root.invalidateSidebar(0) }
        function onFile_filter_directoriesChanged() { root.invalidateSidebar(0) }
        function onSearch_result_idsChanged() { root.invalidateSidebar(1) }
        function onSearch_result_linesChanged() { root.invalidateSidebar(1) }
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

    // Smoke tests need the filtered tree before returning.
    function filterNamesNow(query) {
        fileNameQuery = query
        controller.filter_files_now(query)
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
        if (sidebarTab === 0) {
            if (sidebarEntriesDirty) rebuildOrderedEntries()
            fileTree.forceLayout()
            var row = treeRowForPath(path)
            if (row < 0) return
            fileTree.edit(fileTree.index(row, 0))
        }
        Qt.callLater(root.focusInlineRename)
    }

    function clickSidebarEntry(path, directory) {
        if (renamePath.length > 0 || draggingPath.length > 0) return
        if (directory) {
            selectedDirectoryPath = path
            toggleDirectory(path)
            return
        }
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
            var field = root.Window.window ? root.Window.window.activeFocusItem : null
            if (!field || typeof field.select !== "function" || field === editor) return
            if (activeRenameEditor === field && field.activeFocus) return
            activeRenameEditor = field
            activeRenameEditor.objectName = "sidebarRenameField"
        }
        activeRenameEditor.text = renameName
        activeRenameEditor.forceActiveFocus(Qt.OtherFocusReason)
        var dot = controller.directory_ids.indexOf(renamePath) === -1 ? renameName.lastIndexOf(".") : -1
        activeRenameEditor.select(0, dot > 0 ? dot : renameName.length)
    }

    function commitRename() {
        if (renamePath.length === 0 || !flushEditor()) return
        var path = renamePath
        var name = renameName
        if (sidebarTab === 0) fileTree.closeEditor()
        controller.rename_entry(path, name)
        if (controller.error_message.length === 0) renamePath = ""
    }

    function parentDirectory(path) { return path.split("/").slice(0, -1).join("/") }

    function isPendingMove(path) {
        var target = controller.pending_move_target
        return target && (path === target || path.startsWith(target + "/"))
    }

    function canDrop(source, directory) {
        if (controller.move_pending) return false
        if (source.length === 0) return false
        var target = (directory.length > 0 ? directory + "/" : "") + source.split("/").pop()
        if (target === source || target.startsWith(source + "/")) return false
        // The Files tree indexes every entry by parent. Reading the full id
        // lists copies the whole workspace into JS on each pointer move.
        if (lazyTree() && !sidebarEntriesDirty)
            return !childEntries(directory).some(entry => entry.path === target)
        return controller.directory_ids.indexOf(target) === -1
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
            if (sidebarTab === 0 && fileFilter.length === 0 && treeRowForPath(path) >= 0) {
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
        temporaryDragDirectories = []
        dragPreviewVisible = false
        controller.drop_preview_hidden()
        hoveringDrop = false
        validDrop = false
        // End hit testing and hover feedback before replacing tree rows.
        dropCommitted = true
        committedDropDirectory = directory
        finishEntryDrag(true)
        // This runs inside the DragHandler release and DropArea signal of
        // tree delegates. Replacing rows here pools or destroys them while Qt
        // is still delivering the release, so the model changes next turn.
        Qt.callLater(root.commitDrop, source, directory, temporaryFolders)
        return true
    }

    function commitDrop(source, directory, temporaryFolders) {
        temporaryDragDirectories = temporaryFolders
        restoreDragDirectories(directory)
        temporaryFolders = temporaryDragDirectories
        temporaryDragDirectories = []
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
        dragGhost.x = point.x + Kirigami.Units.smallSpacing
        dragGhost.y = point.y + Kirigami.Units.smallSpacing
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
        onActivated: { fileTree.closeEditor(); root.renamePath = ""; root.finishEntryDrag(true) }
    }

    Kirigami.AbstractCard {
        id: dragGhost
        objectName: "sidebarDragGhost"
        z: 1000
        visible: root.dragPreviewVisible
        width: implicitWidth
        height: implicitHeight
        Drag.active: root.draggingPath.length > 0
        Drag.source: dragGhost
        Drag.keys: ["native-workspace-entry"]
        Drag.hotSpot.x: -Kirigami.Units.smallSpacing
        Drag.hotSpot.y: -Kirigami.Units.smallSpacing
        Drag.supportedActions: Qt.MoveAction
        Drag.proposedAction: Qt.MoveAction
        contentItem: Kirigami.IconTitleSubtitle {
            title: root.draggingPath.split("/").pop()
            icon.source: root.entryIcon(root.draggingPath,
                root.controller.directory_ids.indexOf(root.draggingPath) !== -1,
                root.collapsedDirectories[root.draggingPath] === false)
            icon.color: "transparent"
        }
    }

    Kirigami.Dialog {
        id: createEntryDialog
        preferredWidth: Kirigami.Units.gridUnit * 24
        title: root.creatingDirectory ? "New Folder" : "New File"
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        Kirigami.FormLayout {
            TextField {
                id: newEntryPath
                Kirigami.FormData.label: "Relative path:"
                placeholderText: root.creatingDirectory ? "folder" : "file.txt"
                onAccepted: createEntryDialog.accept()
            }
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
        // The controller filters names on a worker; see filter_files.
        return Array.from(fileFilter.length > 0 ? controller.file_filter_files : controller.document_ids)
    }

    function entryIcon(path, directory, expanded) {
        return directory ? controller.folder_icon(path, expanded) : controller.file_icon(path)
    }

    // The plain Files tree asks the core for one folder at a time, so
    // collapsed dependency folders cost nothing. Searches and name filters
    // still list every path.
    function lazyTree() { return sidebarTab === 0 && fileFilter.length === 0 }

    function childEntries(parent) {
        var key = "#" + parent
        var listed = childEntriesByParent[key]
        if (listed || !lazyTree()) return listed || []
        listed = controller.sidebar_children(parent).map(item => ({ path: item.substring(1), directory: item[0] === "d" }))
        childEntriesByParent[key] = listed
        return listed
    }

    // Collapsed folders hold one placeholder row instead of their subtree, so
    // dependency trees such as node_modules do not enter the native model
    // until opened. Converting every row took seconds after each move.
    function buildTreeRows(parent, parentIndex, indexPaths, complete) {
        return childEntries(parent).map(function(entry, row) {
            var indexPath = parentIndex.concat([row])
            indexPaths["#" + entry.path] = indexPath
            var node = {
                name: entry.path.split("/").pop(),
                path: entry.path,
                directory: entry.directory
            }
            if (entry.directory) {
                var open = complete || collapsedDirectories[entry.path] === false
                if (open) node.rows = buildTreeRows(entry.path, indexPath, indexPaths, complete)
                else if (lazyTree() ? controller.sidebar_has_children(entry.path)
                         : (childEntriesByParent["#" + entry.path] || []).length > 0)
                    node.rows = [{ name: "", path: entry.path, directory: false, placeholder: true }]
            }
            return node
        })
    }

    function materializeTreeFolder(path) {
        var indexPath = treeIndexPaths["#" + path]
        if (!indexPath || childEntries(path).length === 0) return
        var first = fileTreeModel.getRow(fileTreeModel.index(indexPath.concat([0]), 0))
        if (!first || !first.placeholder) return
        var parentIndex = fileTreeModel.index(indexPath, 0)
        var rows = buildTreeRows(path, indexPath, treeIndexPaths, false)
        fileTreeModel.removeRow(fileTreeModel.index(indexPath.concat([0]), 0))
        for (var row of rows) fileTreeModel.appendRow(parentIndex, row)
    }

    function treeEntry(modelIndex) {
        var entry = fileTreeModel.getRow(nativeTreeModel.mapToSource(modelIndex))
        return entry && !entry.placeholder ? entry : null
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
        if (lazyTree()) {
            childEntriesByParent = ({})
            var lazyIndexPaths = ({})
            restoringFileTree = true
            fileTreeNeedsRestore = true
            var lazyRows = buildTreeRows("", [], lazyIndexPaths, false)
            treeIndexPaths = lazyIndexPaths
            fileTreeModel.rows = lazyRows
            Qt.callLater(root.restoreFileTreeExpansion)
            var openEntries = []
            var pending = [""]
            // Depth-first over open folders, preserving each folder's order.
            while (pending.length > 0) {
                var next = pending.pop()
                if (typeof next !== "string") { openEntries.push(next); continue }
                var listed = root.childEntries(next)
                for (var i = listed.length - 1; i >= 0; --i) {
                    if (listed[i].directory && root.collapsedDirectories[listed[i].path] === false)
                        pending.push(listed[i].path)
                    pending.push(listed[i])
                }
            }
            orderedSidebarEntries = openEntries
            updateVisibleEntries()
            cacheCurrentSidebar()
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
        if (sidebarTab === 0) {
            // A published name filter already lists every folder holding a match.
            if (fileFilter.length > 0) workspaceDirectories = root.controller.file_filter_directories
            for (var k = 0; k < workspaceDirectories.length; ++k) {
                var directory = workspaceDirectories[k]
                directories.push(directory)
                seenDirectories["#" + directory] = true
            }
        }
        if (sidebarTab === 1) {
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
            restoringFileTree = true
            fileTreeNeedsRestore = true
            var indexPaths = ({})
            var rootRows = buildTreeRows("", [], indexPaths, fileFilter.length > 0)
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
        var searching = sidebarTab === 1 || fileFilter.length > 0
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
            if (!indexPath || fileTree.rowAtIndex(nativeTreeModel.mapFromSource(fileTreeModel.index(indexPath, 0))) !== row)
                return false
        }
        return true
    }

    function treeRowForPath(path) {
        var indexPath = treeIndexPaths["#" + path]
        return indexPath ? fileTree.rowAtIndex(nativeTreeModel.mapFromSource(fileTreeModel.index(indexPath, 0))) : -1
    }

    function restoreFileTreeSelection() {
        if (sidebarTab !== 0 || renamePath.length > 0) return
        var path = selectedDirectoryPath || controller.active_document_id
        var row = treeRowForPath(path)
        if (row >= 0)
            fileTree.selectionModel.setCurrentIndex(fileTree.index(row, 0), ItemSelectionModel.ClearAndSelect)
    }

    function restoreFileTreeExpansion() {
        if (sidebarTab !== 0) {
            restoringFileTree = false
            return
        }
        if (sidebarTab === 0) {
            if (fileFilter.length > 0) {
                fileTree.expandRecursively(-1)
            } else {
                function expandChildren(parent) {
                    for (var entry of root.childEntries(parent)) {
                        if (!entry.directory || collapsedDirectories[entry.path] !== false) continue
                        var modelIndex = fileTreeModel.index(treeIndexPaths["#" + entry.path], 0)
                        var row = fileTree.rowAtIndex(nativeTreeModel.mapFromSource(modelIndex))
                        if (row >= 0 && !fileTree.isExpanded(row)) fileTree.expand(row)
                        expandChildren(entry.path)
                    }
                }
                expandChildren("")
            }
        }
        fileTreeNeedsRestore = false
        restoringFileTree = false
        restoreFileTreeSelection()
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
            for (var entry of root.childEntries(parent)) {
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
        if (sidebarTab === 0 && fileFilter.length === 0 && treeIndexPaths["#" + path]) {
            if (expanding) materializeTreeFolder(path)
            var treeRow = fileTree.rowAtIndex(nativeTreeModel.mapFromSource(fileTreeModel.index(treeIndexPaths["#" + path], 0)))
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

    // Confirm a move as soon as its worker finishes, so a completed move
    // never looks like it is still loading.
    Timer {
        interval: 16
        repeat: true
        running: root.controller && root.controller.move_pending
        onTriggered: if (root.flushEditor()) {
            root.controller.poll_workspace_changes()
            root.controller.poll_search_results()
        }
    }

    // Searches and name filters run on workers; publish their results
    // promptly instead of on the next slow poll.
    Timer {
        interval: 16
        repeat: true
        running: root.controller && (root.controller.search_in_progress || root.controller.file_filter_in_progress)
        onTriggered: {
            root.controller.poll_file_filter()
            if (root.controller.search_in_progress && root.flushEditor())
                root.controller.poll_search_results()
        }
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


    readonly property list<Action> applicationActions: [homeAction, openAction, developerAction, quitAction]
    readonly property list<Action> pageActions: [saveAction, newFileAction, newFolderAction]
    readonly property var syntaxColors: [Kirigami.Theme.textColor, Kirigami.Theme.linkColor,
        Kirigami.Theme.neutralTextColor, Kirigami.Theme.positiveTextColor,
        Kirigami.Theme.disabledTextColor, Kirigami.Theme.activeTextColor]
    onSyntaxColorsChanged: Qt.callLater(function() {
        lineNumbers.requestPaint()
        minimap.requestPaint()
    })

    Kirigami.Action {
        id: homeAction
        text: "Home"
        icon.name: "go-home"
        onTriggered: if (root.flushEditor()) root.showingHome = true
    }
    Kirigami.Action {
        id: openAction
        text: "Open Folder…"
        icon.name: "document-open-folder"
        shortcut: StandardKey.Open
        onTriggered: root.browseRequested()
    }
    Kirigami.Action {
        id: resumeAction
        text: "Resume Workspace"
        icon.name: "go-next"
        visible: root.controller.workspace_root.length > 0
        onTriggered: root.showingHome = false
    }
    Kirigami.Action {
        id: saveAction
        text: "Save"
        icon.name: "document-save"
        enabled: root.controller.active_document_dirty && !root.controller.active_document_external_change
        onTriggered: root.saveActive()
    }
    Kirigami.Action {
        id: newFileAction
        text: "New File…"
        icon.name: "document-new"
        onTriggered: { root.creatingDirectory = false; createEntryDialog.open() }
    }
    Kirigami.Action {
        id: newFolderAction
        text: "New Folder…"
        icon.name: "folder-new"
        onTriggered: { root.creatingDirectory = true; createEntryDialog.open() }
    }
    Kirigami.Action {
        id: developerAction
        text: "Developer Tools"
        icon.name: "tools-report-bug"
        visible: root.controller.devtools_available
        onTriggered: root.controller.open_devtools()
    }
    Kirigami.Action {
        id: quitAction
        text: "Quit"
        icon.name: "application-exit"
        shortcut: StandardKey.Quit
        onTriggered: Qt.quit()
    }

    Kirigami.ApplicationWindow {
        id: homeWindow
        transientParent: null
        title: "Native Workspace"
        width: Kirigami.Units.gridUnit * 36
        height: Kirigami.Units.gridUnit * 32
        minimumWidth: Kirigami.Units.gridUnit * 24
        minimumHeight: Kirigami.Units.gridUnit * 20
        visible: root.enableBrowse && root.showingHome
        onClosing: Qt.quit()
        globalDrawer: Kirigami.GlobalDrawer {
            title: "Native Workspace"
            titleIcon: "accessories-text-editor"
            isMenu: true
            actions: [developerAction, quitAction]
        }
        pageStack.initialPage: Kirigami.ScrollablePage {
            title: "Recent Workspaces"
            actions: [openAction, resumeAction]
            ListView {
                model: root.controller.recent_workspace_paths
                delegate: Kirigami.SubtitleDelegate {
                    required property int index
                    required property string modelData
                    width: ListView.view.width
                    text: root.controller.recent_workspace_names[index]
                    subtitle: modelData
                    icon.source: root.controller.folder_icon(modelData, false)
                    icon.color: "transparent"
                    onClicked: root.openWorkspace(modelData)
                }
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.largeSpacing * 2
                    visible: root.controller.recent_workspace_paths.length === 0
                    icon.name: "document-open-folder"
                    text: "No Recent Workspaces"
                    explanation: "Open a folder to start editing."
                    helpfulAction: openAction
                }
            }
            footer: Kirigami.InlineMessage {
                visible: root.controller.error_message.length > 0
                text: root.controller.error_message
                type: Kirigami.MessageType.Error
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.controller.workspace_name.length > 0
        Kirigami.PageRow {
            id: workspacePages
            Layout.fillWidth: true
            Layout.fillHeight: true
            globalToolBar.style: Kirigami.ApplicationHeaderStyle.ToolBar
            leftSidebar: Kirigami.OverlayDrawer {
                id: workspaceSidebar
                objectName: "workspaceSidebar"
                modal: false
                drawerOpen: true
                handleVisible: false
                interactiveResizeEnabled: true
                preferredSize: Kirigami.Units.gridUnit * 18
                minimumSize: Kirigami.Units.gridUnit * 12
                padding: 0
                Kirigami.Theme.highlightColor: root.Kirigami.Theme.highlightColor
                Kirigami.Theme.highlightedTextColor: root.Kirigami.Theme.highlightedTextColor
                contentItem: ColumnLayout {
                    spacing: 0
                    ToolBar {
                        id: sidebarTopBar
                        objectName: "sidebarTopBar"
                        Layout.fillWidth: true
                        Layout.preferredHeight: workspacePages.globalToolBar.preferredHeight
                        contentItem: Kirigami.Heading {
                            text: root.sidebarSections[root.sidebarTab].title
                            level: 2
                            elide: Text.ElideRight
                        }
                    }
                    Pane {
                        id: sidebarPane
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        padding: 0
                        Kirigami.Theme.colorSet: Kirigami.Theme.View
                        Item {
                            anchors.fill: parent
                            ColumnLayout {
                                id: sidebarHeader
                                anchors.top: parent.top
                                anchors.left: parent.left
                                anchors.right: parent.right
                                spacing: Kirigami.Units.smallSpacing
                                TabBar {
                                    id: sidebarTabs
                                    objectName: "sidebarTabs"
                                    Layout.fillWidth: true
                                    currentIndex: root.sidebarTab
                                    onCurrentIndexChanged: if (currentIndex >= 0) {
                                        root.sidebarTab = currentIndex
                                        root.selectedResultIndex = -1
                                    }
                                    Repeater {
                                        model: root.sidebarSections
                                        delegate: Kirigami.NavigationTabButton {
                                            required property int index
                                            required property var modelData
                                            text: modelData.title
                                            icon.name: modelData.icon
                                            display: AbstractButton.IconOnly
                                            checked: root.sidebarTab === index
                                            ToolTip.visible: hovered
                                            ToolTip.text: text
                                            ToolTip.delay: Kirigami.Units.toolTipDelay
                                        }
                                    }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    Layout.margins: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === 0
                                    Kirigami.SearchField {
                                        objectName: "fileNameFilter"
                                        Layout.fillWidth: true
                                        text: root.fileNameQuery
                                        placeholderText: "Filter names…"
                                        onTextEdited: { root.selectedResultIndex = -1; root.fileNameQuery = text }
                                    }
                                    BusyIndicator {
                                        running: root.controller.file_filter_in_progress
                                        visible: running
                                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                                    }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    Layout.margins: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === 1
                                    Kirigami.SearchField {
                                        Layout.fillWidth: true
                                        text: root.controller.search_query
                                        placeholderText: "Search in files…"
                                        onTextEdited: { root.selectedResultIndex = -1; root.controller.search_files(text) }
                                    }
                                    BusyIndicator {
                                        running: root.controller.search_in_progress
                                        visible: running
                                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                                    }
                                }
                                Label {
                                    objectName: "searchTruncated"
                                    Layout.fillWidth: true
                                    Layout.leftMargin: Kirigami.Units.smallSpacing
                                    Layout.rightMargin: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === 1 && root.controller.search_truncated
                                    text: "Showing the first 20,000 matches. Refine the search to see the rest."
                                    wrapMode: Text.Wrap
                                    font: Kirigami.Theme.smallFont
                                    opacity: 0.7
                                }
                            }
                            Item {
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                visible: root.sidebarTab >= 2
                                Kirigami.PlaceholderMessage {
                                    anchors.centerIn: parent
                                    width: parent.width - Kirigami.Units.largeSpacing * 2
                                    text: root.sidebarSections[root.sidebarTab]?.title ?? ""
                                    icon.name: root.sidebarSections[root.sidebarTab]?.icon ?? ""
                                    explanation: root.sidebarSections[root.sidebarTab]?.explanation ?? ""
                                }
                            }
                            TreeView {
                                id: fileTree
                                objectName: "fileTree"
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: workspaceDropTarget.top
                                anchors.bottomMargin: Kirigami.Units.smallSpacing
                                visible: root.sidebarTab === 0
                                clip: true
                                model: nativeTreeModel
                                selectionModel: ItemSelectionModel { model: nativeTreeModel }
                                selectionBehavior: TableView.SelectRows
                                selectionMode: TableView.SingleSelection
                                editTriggers: TableView.EditKeyPressed
                                Keys.onPressed: function(event) {
                                    if (event.key !== Qt.Key_F2) return
                                    var entry = root.treeEntry(selectionModel.currentIndex)
                                    if (!entry) return
                                    root.beginRename(entry.path)
                                    event.accepted = true
                                }
                                ScrollBar.vertical: ScrollBar { }
                                columnWidthProvider: function() { return width }
                                onWidthChanged: forceLayout()
                                onExpanded: function(row) {
                                    if (root.sidebarTab !== 0 || root.restoringFileTree || root.fileFilter.length > 0) return
                                    var entry = root.treeEntry(fileTree.index(row, 0))
                                    if (entry && root.collapsedDirectories[entry.path] !== false)
                                        root.toggleDirectory(entry.path)
                                }
                                onCollapsed: function(row) {
                                    if (root.sidebarTab !== 0 || root.restoringFileTree || root.fileFilter.length > 0) return
                                    var entry = root.treeEntry(fileTree.index(row, 0))
                                    if (entry && root.collapsedDirectories[entry.path] === false)
                                        root.toggleDirectory(entry.path)
                                }
                                Kirigami.PlaceholderMessage {
                                    anchors.centerIn: parent
                                    width: parent.width - Kirigami.Units.largeSpacing * 2
                                    icon.name: "folder"
                                    visible: root.visibleSidebarEntries.length === 0 && !root.controller.file_filter_in_progress
                                    text: root.fileFilter.length > 0 ? "No matching names" : "No files"
                                }
                                delegate: TreeViewDelegate {
                                    id: treeRow
                                    text: model.display
                                    icon.source: entry ? root.entryIcon(entry.path, entry.directory, expanded) : ""
                                    icon.color: "transparent"
                                    icon.width: Kirigami.Units.iconSizes.small
                                    icon.height: Kirigami.Units.iconSizes.small
                                    contentItem: Kirigami.IconTitleSubtitle {
                                        title: treeRow.text
                                        icon: icon.fromControlsIcon(treeRow.icon)
                                        font: treeRow.font
                                        selected: treeRow.highlighted
                                        visible: !treeRow.editing
                                    }
                                    // Qt's fallback style reads QQuickPalette instead of
                                    // Kirigami.Theme. Bridge the view's theme roles so a
                                    // different macOS appearance cannot hide file names.
                                    palette {
                                        windowText: treeRow.highlighted ? systemColors.highlightedText : treeRow.Kirigami.Theme.textColor
                                        text: treeRow.Kirigami.Theme.textColor
                                        base: treeRow.Kirigami.Theme.backgroundColor
                                        alternateBase: treeRow.Kirigami.Theme.alternateBackgroundColor
                                        accent: systemColors.accent
                                        highlight: systemColors.accent
                                        highlightedText: systemColors.highlightedText
                                        inactive.highlight: systemColors.accent
                                        inactive.highlightedText: systemColors.highlightedText
                                    }
                                    property bool pooled: false
                                    TableView.onPooled: pooled = true
                                    TableView.onReused: pooled = false
                                    // Expanding changes the mapping of view rows even
                                    // when a reused delegate keeps the same row number.
                                    property var entry: row < fileTree.rows && model.display !== undefined
                                        ? root.treeEntry(fileTree.index(row, 0)) : null
                                    property bool pendingMove: entry && root.isPendingMove(entry.path)
                                    property string editingPath: ""
                                    enabled: !pendingMove
                                    onEditingChanged: {
                                        if (editing && entry) {
                                            editingPath = entry.path
                                            root.renamePath = entry.path
                                            root.renameName = entry.name
                                            Qt.callLater(root.focusInlineRename)
                                        } else if (root.renamePath === editingPath) {
                                            root.renamePath = ""
                                            root.activeRenameEditor = null
                                            editingPath = ""
                                        }
                                    }
                                    ToolTip.visible: hovered
                                    ToolTip.text: entry ? (pendingMove ? "Moving… Waiting for confirmation" : entry.path) : ""
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
                                            dragGhost.x = point.x + Kirigami.Units.smallSpacing
                                            dragGhost.y = point.y + Kirigami.Units.smallSpacing
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
                                    onClicked: if (entry) {
                                        if (entry.directory) root.selectedDirectoryPath = entry.path
                                        else root.clickSidebarEntry(entry.path, false)
                                    }
                                }
                            }
                            Kirigami.AbstractCard {
                                id: workspaceDropTarget
                                objectName: "workspaceDropTarget"
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                height: visible ? implicitHeight : 0
                                visible: root.sidebarTab === 0 && root.draggingPath.length > 0
                                text: root.hoveringDrop && !root.validDrop ? "This folder cannot receive the item" : "Move to workspace root"
                                contentItem: Kirigami.IconTitleSubtitle {
                                    title: workspaceDropTarget.text
                                    icon.name: root.hoveringDrop && !root.validDrop ? "dialog-warning" : "go-up"
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
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                visible: root.sidebarTab === 1
                                clip: true
                                model: sidebarModel
                                reuseItems: true
                                ScrollBar.vertical: ScrollBar { }
                                Kirigami.PlaceholderMessage {
                                    anchors.centerIn: parent
                                    width: parent.width - Kirigami.Units.largeSpacing * 2
                                    text: root.controller.search_query.length > 0 ? "No text matches" : "Search in Files"
                                    explanation: root.controller.search_query.length > 0 ? "Try a different search." : "Enter text to search this workspace."
                                    icon.name: "edit-find"
                                    visible: root.visibleSidebarEntries.length === 0 && !root.controller.search_in_progress
                                }
                                delegate: Column {
                                    id: fileGroup
                                    width: ListView.view.width
                                    property string fileId: model.path
                                    property bool isDirectory: model.directory
                                    // Matches nest under their file, as in the Files tree.
                                    property real indent: Kirigami.Units.smallSpacing
                                        + (fileGroup.fileId.split("/").length - 1) * Kirigami.Units.gridUnit
                                    Kirigami.SubtitleDelegate {
                                        id: searchFileDelegate
                                        objectName: "sidebarSearchFile:" + fileGroup.fileId
                                        width: parent.width
                                        leftPadding: fileGroup.indent
                                        text: root.renamePath === fileGroup.fileId ? "" : fileGroup.fileId.split("/").pop()
                                        icon.source: root.entryIcon(fileGroup.fileId, fileGroup.isDirectory, true)
                                        icon.color: "transparent"
                                        rightPadding: searchStatus.implicitWidth + Kirigami.Units.smallSpacing
                                        highlighted: fileGroup.isDirectory ? root.selectedDirectoryPath === fileGroup.fileId
                                            : root.selectedDirectoryPath.length === 0 && root.selectedResultIndex < 0 && root.controller.active_document_id === fileGroup.fileId
                                        onClicked: root.clickSidebarEntry(fileGroup.fileId, fileGroup.isDirectory)
                                        Row {
                                            id: searchStatus
                                            anchors.right: parent.right
                                            anchors.rightMargin: Kirigami.Units.smallSpacing
                                            anchors.verticalCenter: parent.verticalCenter
                                            spacing: Kirigami.Units.smallSpacing
                                            Kirigami.Icon {
                                                source: "dialog-warning"
                                                visible: !fileGroup.isDirectory && root.controller.conflicted_document_ids.indexOf(fileGroup.fileId) !== -1
                                                width: Kirigami.Units.iconSizes.small
                                                height: width
                                                Accessible.name: "File changed on disk"
                                            }
                                            Kirigami.Icon {
                                                source: "document-edit"
                                                visible: !fileGroup.isDirectory && root.controller.dirty_document_ids.indexOf(fileGroup.fileId) !== -1
                                                width: Kirigami.Units.iconSizes.small
                                                height: width
                                                Accessible.name: "Unsaved changes"
                                            }
                                        }
                                        TapHandler {
                                            acceptedButtons: Qt.RightButton
                                            onTapped: searchEntryMenu.popup()
                                        }
                                        Menu {
                                            id: searchEntryMenu
                                            MenuItem {
                                                text: "Rename…"
                                                icon.name: "edit-rename"
                                                onTriggered: root.beginRename(fileGroup.fileId)
                                            }
                                        }
                                        TextField {
                                            id: searchRenameField
                                            objectName: "sidebarRenameField"
                                            visible: root.renamePath === fileGroup.fileId
                                            anchors.left: parent.left
                                            anchors.leftMargin: parent.leftPadding + parent.icon.width + parent.spacing
                                            anchors.right: parent.right
                                            anchors.rightMargin: parent.rightPadding
                                            anchors.verticalCenter: parent.verticalCenter
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
                                    }
                                    Repeater {
                                        model: fileGroup.isDirectory || root.controller.search_query.length === 0
                                            ? [] : root.matchIndicesForFile(fileGroup.fileId)
                                        delegate: Kirigami.SubtitleDelegate {
                                            property int resultIndex: modelData
                                            objectName: "sidebarSearchMatch:" + resultIndex
                                            width: fileGroup.width
                                            leftPadding: fileGroup.indent + Kirigami.Units.gridUnit
                                            text: root.controller.search_result_snippets[resultIndex] || ""
                                            subtitle: "Line " + root.controller.search_result_lines[resultIndex] + ", Column " + root.controller.search_result_columns[resultIndex]
                                            icon.name: "go-jump"
                                            highlighted: root.selectedResultIndex === resultIndex
                                            onClicked: root.openSearchResult(resultIndex, fileGroup.fileId)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            initialPage: Kirigami.Page {
                id: editorPage
                objectName: "workspaceEditorPage"
                title: root.controller.workspace_name
                actions: root.pageActions
                padding: 0
                ColumnLayout {
                    anchors.fill: parent
                    spacing: Kirigami.Units.smallSpacing
                    Pane {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        padding: 0
                        Kirigami.Theme.colorSet: Kirigami.Theme.View
                        RowLayout {
                            anchors.fill: parent
                            spacing: 0
                            FontMetrics { id: editorMetrics; font: editor.font }
                            FontInfo { id: resolvedEditorFont; font: editor.font }
                            Canvas {
                                id: lineNumbers
                                objectName: "editorLineNumbers"
                                Layout.preferredWidth: editorMetrics.averageCharacterWidth * Math.max(3, String(lineStarts.length).length) + Kirigami.Units.largeSpacing * 2
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
                                    ctx.fillStyle = Kirigami.Theme.disabledTextColor
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
                                        ctx.fillText(String(line + 1), width - Kirigami.Units.largeSpacing, numberBaseline(line))
                                    }
                                }
                                MouseArea {
                                    anchors.fill: parent
                                    enabled: editor.enabled
                                    cursorShape: Qt.IBeamCursor
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
                                TextArea {
                                    id: editor
                                    objectName: "documentEditor"
                                    width: Math.max(editorScroll.availableWidth, implicitWidth)
                                    height: Math.max(editorScroll.availableHeight, implicitHeight)
                                    readonly property color activeLineColor: Qt.rgba(systemColors.accent.r,
                                        systemColors.accent.g, systemColors.accent.b, 0.15)
                                    // Some styles (including Fusion) have no TextArea background.
                                    // Draw behind the text in that case; retain native backgrounds otherwise.
                                    Item {
                                        parent: editor.background || editor
                                        z: editor.background ? 0 : -1
                                        anchors.fill: parent
                                        clip: true
                                        Rectangle {
                                            id: activeLine
                                            objectName: "editorActiveLine"
                                            y: editor.cursorRectangle.y
                                                - (editor.background ? editorScroll.contentItem.contentY : 0)
                                            width: editor.width
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
                                        colors: root.syntaxColors
                                    }
                                    enabled: root.controller.active_document_id.length > 0
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
                                objectName: "editorMinimap"
                                Layout.preferredWidth: Kirigami.Units.gridUnit * 5
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
                                    var palette = root.syntaxColors
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
                                    color: "transparent"
                                    border.color: Kirigami.Theme.highlightColor
                                }
                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
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
                        Kirigami.PlaceholderMessage {
                            anchors.centerIn: parent
                            visible: root.controller.active_document_id.length === 0
                            icon.name: "document-edit"
                            text: "No Document Open"
                            explanation: "Choose a file from the sidebar."
                        }
                    }
                    Kirigami.InlineMessage {
                        Layout.fillWidth: true
                        visible: root.controller.active_document_external_change
                        text: "This file changed on disk. Reload it or save your edits over the disk version."
                        type: Kirigami.MessageType.Warning
                        actions: [
                            Kirigami.Action {
                                text: "Reload (discard edits)"
                                icon.name: "view-refresh"
                                onTriggered: root.controller.reload_document()
                            },
                            Kirigami.Action {
                                text: "Overwrite Disk"
                                icon.name: "document-save"
                                onTriggered: if (root.flushEditor()) root.controller.overwrite_document()
                            }
                        ]
                    }
                    Kirigami.InlineMessage {
                        Layout.fillWidth: true
                        visible: root.controller.error_message.length > 0
                        text: root.controller.error_message
                        type: Kirigami.MessageType.Error
                    }
                }
            }
        }
        Kirigami.Separator { Layout.fillWidth: true }
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.largeSpacing
            BusyIndicator {
                objectName: "pendingMoveIndicator"
                visible: root.controller.pending_move_target.length > 0
                running: visible
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                Accessible.name: "Moving… Waiting for confirmation"
            }
            Label {
                text: root.controller.active_document_id || "No document"
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }
            Label {
                text: root.controller.active_document_id.length === 0 ? "Ready" : (root.controller.active_document_dirty ? "Modified" : "Saved")
            }
        }
    }

    Connections {
        target: root.controller
        function onActive_document_idChanged() {
            Qt.callLater(root.updateHighlight)
            Qt.callLater(root.restoreFileTreeSelection)
        }
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
