import QtQuick
import org.nativeworkspace.Desktop
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.qmlmodels
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Syntax 1.0
import org.nativeworkspace.Editor 1.0

Item {
    id: root
    enabled: !controller || (!controller.editing_paused && !terminalQuitPending)
    SearchResultsModel { id: searchResults; rows: root.controller ? root.controller.search_results : [] }
    SystemPalette { id: systemColors; colorGroup: SystemPalette.Active }
    // Keep selection accented when focus moves to the editor or another window.
    // The host's accent may differ from Breeze's default selection blue.
    Kirigami.Theme.highlightColor: systemColors.accent
    Kirigami.Theme.highlightedTextColor: systemColors.highlightedText
    signal terminalRequested()
    property bool terminalVisible: false
    property var controller
    property bool enableBrowse: false
    property bool showingHome: true
    property alias homeWindow: homeWindow
    property alias sidebar: workspaceSidebar
    property alias sidebarHeaderBar: sidebarTopBar
    property alias workspacePage: editorPage
    property bool creatingDirectory: false
    enum SidebarSection { Files, Search, Git, Bookmarks, Warnings, Errors }
    property int sidebarTab: WorkspacePane.Files
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
        if (controller) controller.set_git_visible(sidebarTab === WorkspacePane.Git)
        if (sidebarTab === WorkspacePane.Git && controller && !controller.move_pending) controller.refresh_git()
        scheduleSidebarUpdate()
        if (sidebarTab === WorkspacePane.Files) Qt.callLater(root.restoreFileTreeSelection)
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
    readonly property real documentTabHeight: Math.max(fileNameFilter.implicitHeight, textSearchField.implicitHeight, Kirigami.Units.iconSizes.smallMedium + Kirigami.Units.smallSpacing * 2)
    property bool synchronizingDocumentTabs: true
    property int selectedResultIndex: -1
    property string selectedDirectoryPath: ""
    readonly property var activeSidebarSearchRow: sidebarTab === WorkspacePane.Files ? fileSearchRow : textSearchRow
    property alias sidebarTabBar: sidebarTabs
    property alias documentTabBar: documentTabBar
    property alias editorText: editor.text
    property alias editorFont: editor.font
    readonly property real editorBaseFontPointSize: EditorFonts.fixedFont.pointSize > 0 ? EditorFonts.fixedFont.pointSize : 12
    property real editorFontPointSize: editorBaseFontPointSize
    readonly property int editorZoomPercent: Math.round(editorFontPointSize / editorBaseFontPointSize * 100)
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
    property var externalDropUrls: []
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
    ListModel { id: documentTabsModel }
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
        function onSearch_resultsChanged() { root.invalidateSidebar(1) }
    }

    Component.onCompleted: {
        syncDocumentTabs()
        scheduleSidebarUpdate()
        minimapGlyphs = root.controller ? root.controller.minimap_glyphs() : []
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
            var span = spans[index]
            var end = Math.min(source.length, span.end)
            for (var position = span.start; position < end; ++position)
                kinds[position] = span.kind
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
        if (sidebarTab >= WorkspacePane.Git) return
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

    function requestQuit() {
        if (!flushEditor()) return
        if (controller.move_pending) {
            controller.prepare_to_quit(false)
        } else if (controller.dirty_document_ids.length > 0) {
            quitDialog.open()
        } else {
            finishQuit(false)
        }
    }

    function finishQuit(save) {
        if (flushEditor() && controller.prepare_to_quit(save)) requestTerminalQuit()
    }

    Connections {
        target: ApplicationLifecycle
        function onQuitRequested() { root.requestQuit() }
    }

    Kirigami.Dialog {
        id: quitDialog
        objectName: "quitDialog"
        parent: root.showingHome ? homeWindow.Overlay.overlay : root.Overlay.overlay
        modal: true
        title: root.controller.quit_confirmation().title
        preferredWidth: Kirigami.Units.gridUnit * 26
        standardButtons: Kirigami.Dialog.NoButton
        Label {
            text: root.controller.quit_confirmation().body
            wrapMode: Text.WordWrap
        }
        customFooterActions: [
            Kirigami.Action { objectName: "cancelQuitAction"; text: root.controller.quit_confirmation().cancel; onTriggered: quitDialog.close() },
            Kirigami.Action { objectName: "discardQuitAction"; text: root.controller.quit_confirmation().discard; onTriggered: { quitDialog.close(); root.finishQuit(false) } },
            Kirigami.Action { objectName: "saveQuitAction"; text: root.controller.quit_confirmation().confirm; onTriggered: { quitDialog.close(); root.finishQuit(true) } }
        ]
    }

    function flushEditor() {
        if (controller.active_document_id.length > 0 && editor.text !== controller.active_document_text) {
            controller.change_document(editor.text)
            return controller.error_message.length === 0
        }
        return true
    }

    Shortcut {
        sequence: "Ctrl+Shift+S"
        onActivated: if (root.flushEditor()) root.controller.save_all_documents()
    }

    function syncDocumentTabs() {
        if (!controller || controller.snapshot_updating) return
        synchronizingDocumentTabs = true
        var paths = controller.open_document_ids
        var previous = []
        for (var i = 0; i < documentTabsModel.count; i++) previous.push(documentTabsModel.get(i).path)
        // Relocations retain the same tab object and its position.
        if (previous.length === paths.length) {
            for (var i = 0; i < paths.length; i++) {
                if (paths.indexOf(previous[i]) === -1 && previous.indexOf(paths[i]) === -1)
                    documentTabsModel.setProperty(i, "path", paths[i])
            }
        }
        var existing = []
        for (var i = 0; i < documentTabsModel.count; i++) existing.push(documentTabsModel.get(i).path)
        for (var path of paths) {
            if (existing.indexOf(path) === -1) {
                documentTabsModel.append({path: path})
                existing.push(path)
            }
        }
        // Select the final destination before removing an active old tab, so
        // TabBar never needs to choose a temporary first-tab fallback.
        documentTabBar.setCurrentIndex(existing.indexOf(controller.active_document_id))
        for (var i = documentTabsModel.count - 1; i >= 0; i--) {
            if (paths.indexOf(documentTabsModel.get(i).path) === -1) documentTabsModel.remove(i)
        }
        for (var i = 0; i < paths.length; i++) {
            if (documentTabsModel.get(i).path === paths[i]) continue
            for (var j = i + 1; j < documentTabsModel.count; j++) {
                if (documentTabsModel.get(j).path === paths[i]) {
                    documentTabsModel.move(j, i, 1)
                    break
                }
            }
        }
        documentTabBar.setCurrentIndex(paths.indexOf(controller.active_document_id))
        synchronizingDocumentTabs = false
    }

    function selectDocumentTab(path) {
        if (!controller.snapshot_updating && !synchronizingDocumentTabs && flushEditor())
            controller.preview_document(path)
        syncDocumentTabs()
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
        if (sidebarTab === WorkspacePane.Files) {
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
            controller.preview_document(path)
            selectedResultIndex = -1
        }
    }

    Connections {
        target: root.Window.window
        function onActiveFocusItemChanged() {
            // The native edit delegate can be created after the next event
            // turn, especially when a cached TreeView is shown again.
            if (root.renamePath.length > 0 && root.sidebarTab === WorkspacePane.Files)
                Qt.callLater(root.focusInlineRename)
        }
    }

    function focusInlineRename() {
        if (sidebarTab === WorkspacePane.Search) {
            if (!activeRenameEditor || !activeRenameEditor.visible) return
            if (activeRenameEditor.activeFocus) return
        } else {
            var field = root.Window.window ? root.Window.window.activeFocusItem : null
            if (!field || typeof field.select !== "function") return
            var owner = field
            while (owner && owner !== fileTree) owner = owner.parent
            if (owner !== fileTree) return
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
        if (sidebarTab === WorkspacePane.Files) fileTree.closeEditor()
        controller.rename_entry(path, name)
        if (controller.error_message.length === 0) renamePath = ""
    }

    function parentDirectory(path) { return path.split("/").slice(0, -1).join("/") }

    function isPendingMove(path) {
        var target = controller.pending_move_target
        return target && (path === target || path.startsWith(target + "/"))
    }

    function canDrop(source, directory) {
        if (externalDropUrls.length > 0) return controller.can_move_files(externalDropUrls, directory)
        return controller.can_move_entry(source, directory)
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
            if (sidebarTab === WorkspacePane.Files && fileFilter.length === 0 && treeRowForPath(path) >= 0) {
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
        if (draggingPath.length === 0 && externalDropUrls.length === 0) return
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

    function enterExternalDrop(drag, directory) {
        if (!drag.hasUrls || draggingPath.length > 0) { drag.accepted = false; return }
        externalDropUrls = drag.urls.map(function(url) { return url.toString() })
        hoverDrop(directory)
        drag.accepted = validDrop
    }
    function dropExternalFiles(drop, directory) {
        var urls = drop.urls.map(function(url) { return url.toString() })
        if (!controller.can_move_files(urls, directory) || !flushEditor()) return
        expandDropFolder.stop()
        leaveDropFolder.stop()
        restoreDragDirectories(directory)
        temporaryDragDirectories = []
        if (directory.length > 0) { var folders = Object.assign({}, collapsedDirectories); folders[directory] = false; collapsedDirectories = folders }
        externalDropUrls = []
        hoveringDrop = false
        validDrop = false
        controller.move_files(urls, directory)
        drop.accept(Qt.MoveAction)
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
        if (sidebarTab === WorkspacePane.Files) {
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
        externalDropUrls = []
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
                root.externalDropUrls = []
                root.restoreDragDirectories(null)
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

    EntryDialogs { id: entryDialogs; workspace: root }
    function requestCloseDocument(path) { entryDialogs.requestCloseDocument(path) }
    function requestDelete(path) {
        if (controller.move_pending) return
        fileTree.closeEditor()
        renamePath = ""
        entryDialogs.requestDelete(path)
    }

    Shortcut {
        sequences: [StandardKey.Save]
        enabled: root.controller && root.controller.active_document_id.length > 0
            && (root.controller.active_document_dirty || root.controller.preview_document_id === root.controller.active_document_id)
            && !root.controller.active_document_external_change
        onActivated: root.saveActive()
    }

    // Qt's portable Ctrl notation becomes Command in nativeText on macOS.
    Shortcut {
        id: saveShortcut
        sequence: "Ctrl+S"
        enabled: false
    }
    Shortcut {
        id: openFolderShortcut
        sequence: "Ctrl+O"
        enabled: false
    }
    Shortcut {
        id: saveAllShortcut
        sequence: saveAllAction.shortcut
        enabled: false
    }

    Shortcut {
        sequence: "Ctrl+Shift+F"
        enabled: root.controller && root.controller.workspace_root.length > 0
        onActivated: { root.sidebarTab = WorkspacePane.Search; textSearchField.forceActiveFocus() }
    }
    Shortcut {
        sequence: "Ctrl+Shift+H"
        enabled: root.controller && root.controller.workspace_root.length > 0
        onActivated: { root.sidebarTab = WorkspacePane.Search; searchControls.focusReplace() }
    }

    function openSearchResult(index, id) {
        selectedDirectoryPath = ""
        if (!flushEditor()) return
        var start = searchResults.get(index).start
        var end = searchResults.get(index).end
        controller.preview_document(id)
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
        var files = []
        var seen = ({})
        for (var i = 0; i < searchResults.count; ++i) {
            var id = searchResults.get(i).id
            if (!seen["#" + id]) { files.push(id); seen["#" + id] = true }
        }
        return files
    }

    function fileIds() {
        if (sidebarTab >= WorkspacePane.Git) return []
        if (sidebarTab === WorkspacePane.Search) return textSearchFileIds()
        // The controller filters names on a worker; see filter_files.
        return Array.from(fileFilter.length > 0 ? controller.file_filter_files : controller.document_ids)
    }

    function gitStatus(path) {
        // Register the snapshot dependency; interpretation stays in Rust.
        var revision = controller.git_json
        return revision.length > 0 ? controller.sidebar_git_status(path) : ""
    }
    function gitColor(status) {
        if (status === "A") return "#4e9a63"
        if (status === "M" || status === "R") return "#c88b30"
        if (status === "D" || status === "U") return "#e05b60"
        if (status === "!") return Kirigami.Theme.disabledTextColor
        return Kirigami.Theme.textColor
    }

    function entryIcon(path, directory, expanded) {
        return directory ? controller.folder_icon(path, expanded) : controller.file_icon(path)
    }

    // The plain Files tree asks the core for one folder at a time, so
    // collapsed dependency folders cost nothing. Searches and name filters
    // still list every path.
    function lazyTree() { return sidebarTab === WorkspacePane.Files && fileFilter.length === 0 }

    function childEntries(parent) {
        var key = "#" + parent
        var listed = childEntriesByParent[key]
        if (listed || !lazyTree()) return listed || []
        listed = controller.sidebar_children(parent)
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
        if (sidebarTab >= WorkspacePane.Git) {
            visibleSidebarEntries = []
            return
        }
        var cached = sidebarCaches[sidebarTab]
        if (cached) {
            orderedSidebarEntries = cached.ordered
            visibleSidebarEntries = cached.visible
            childEntriesByParent = cached.children
            if (sidebarTab === WorkspacePane.Search) matchIndicesByFile = cached.matches
            if (sidebarTab === WorkspacePane.Files && fileTreeNeedsRestore)
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
        var matches = ({})
        var childEntries = ({})
        var paths = []
        // Rust supplies the same filtered tree and sibling order as every frontend.
        for (var row of controller.sidebar_rows(sidebarTab === WorkspacePane.Search, [])) {
            if (row.result >= 0) {
                var resultKey = "#" + row.path
                if (!matches[resultKey]) matches[resultKey] = []
                matches[resultKey].push(row.result)
                continue
            }
            var entry = { path: row.path, directory: row.directory }
            var key = "#" + parentDirectory(row.path)
            if (!childEntries[key]) childEntries[key] = []
            childEntries[key].push(entry)
            paths.push(entry)
        }
        if (sidebarTab === WorkspacePane.Search) matchIndicesByFile = matches
        childEntriesByParent = childEntries
        if (sidebarTab === WorkspacePane.Files) {
            restoringFileTree = true
            fileTreeNeedsRestore = true
            var indexPaths = ({})
            var rootRows = buildTreeRows("", [], indexPaths, fileFilter.length > 0)
            treeIndexPaths = indexPaths
            fileTreeModel.rows = rootRows
            Qt.callLater(root.restoreFileTreeExpansion)
        }
        orderedSidebarEntries = paths
        updateVisibleEntries()
        cacheCurrentSidebar()
    }

    function updateVisibleEntries() {
        var searching = sidebarTab === WorkspacePane.Search || fileFilter.length > 0
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
        if (sidebarTab !== WorkspacePane.Search) {
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
        if (sidebarTab !== WorkspacePane.Files || renamePath.length > 0) return
        var path = selectedDirectoryPath || controller.active_document_id
        var row = treeRowForPath(path)
        if (row >= 0)
            fileTree.selectionModel.setCurrentIndex(fileTree.index(row, 0), ItemSelectionModel.ClearAndSelect)
    }

    function restoreFileTreeExpansion() {
        if (sidebarTab !== WorkspacePane.Files) {
            restoringFileTree = false
            return
        }
        if (sidebarTab === WorkspacePane.Files) {
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
        if (sidebarTab === WorkspacePane.Search) {
            if (oldCount > 0) sidebarModel.remove(index + 1, oldCount)
            for (var row = 0; row < children.length; ++row)
                sidebarModel.insert(index + 1 + row, children[row])
        }
        visibleSidebarEntries = visible.slice(0, index + 1).concat(children, visible.slice(oldEnd))
        if (sidebarTab === WorkspacePane.Search) {
            searchSidebarEntries = visibleSidebarEntries
            invalidateSidebar(0)
        }
        cacheCurrentSidebar()
        if (sidebarTab === WorkspacePane.Files && fileFilter.length === 0 && treeIndexPaths["#" + path]) {
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

    Timer {
        interval: root.controller && (root.controller.move_pending || root.controller.search_in_progress || root.controller.file_filter_in_progress || root.controller.terminals_running) ? 16 : 250
        repeat: true
        running: root.controller !== null && root.controller !== undefined
        onTriggered: if (root.flushEditor()) root.controller.poll()
    }

    readonly property list<Action> applicationActions: [homeAction, openAction, terminalAction, developerAction, quitAction]
    readonly property list<Action> pageActions: [textZoomAction, saveAction, saveAllAction]
    readonly property var syntaxColors: [Kirigami.Theme.textColor, Kirigami.Theme.linkColor,
        Kirigami.Theme.neutralTextColor, Kirigami.Theme.positiveTextColor,
        Kirigami.Theme.disabledTextColor, Kirigami.Theme.activeTextColor]
    onSyntaxColorsChanged: Qt.callLater(function() {
        lineNumbers.requestPaint()
        minimap.requestPaint()
    })

    Kirigami.Action {
        id: terminalAction
        text: "Terminal"
        icon.name: "utilities-terminal"
        shortcut: "Ctrl+`"
        onTriggered: root.terminalRequested()
    }
    Kirigami.Action {
        id: textZoomAction
        text: "Text Zoom"
        icon.name: "zoom-original"
        displayComponent: RowLayout {
            spacing: 0
            ToolButton {
                action: decreaseTextSizeAction
                display: AbstractButton.IconOnly
                ToolTip.visible: hovered
                ToolTip.text: text
            }
            Label {
                objectName: "editorZoomPercent"
                text: root.editorZoomPercent + "%"
                Layout.minimumWidth: Kirigami.Units.gridUnit * 3
                horizontalAlignment: Text.AlignHCenter
                Accessible.name: "Text Zoom " + text
            }
            ToolButton {
                action: increaseTextSizeAction
                display: AbstractButton.IconOnly
                ToolTip.visible: hovered
                ToolTip.text: text
            }
        }
    }
    Kirigami.Action {
        id: decreaseTextSizeAction
        objectName: "decreaseTextSizeAction"
        text: "Decrease Text Size"
        icon.name: "zoom-out"
        displayHint: Kirigami.DisplayHint.IconOnly
        enabled: root.editorFontPointSize > 8
        onTriggered: root.editorFontPointSize = Math.max(8, root.editorFontPointSize - 1)
    }
    Kirigami.Action {
        id: increaseTextSizeAction
        objectName: "increaseTextSizeAction"
        text: "Increase Text Size"
        icon.name: "zoom-in"
        displayHint: Kirigami.DisplayHint.IconOnly
        enabled: root.editorFontPointSize < 40
        onTriggered: root.editorFontPointSize = Math.min(40, root.editorFontPointSize + 1)
    }
    Kirigami.Action {
        id: homeAction
        text: "Home"
        icon.name: "go-home"
        onTriggered: if (root.flushEditor()) root.showingHome = true
    }
    Kirigami.Action {
        id: openAction
        shortcut: StandardKey.Open
        text: "Open Folder…"
        icon.name: "document-open-folder"
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
        id: saveAllAction
        objectName: "saveAllAction"
        shortcut: "Ctrl+Shift+S"
        text: "Save All"
        icon.name: "document-save-all"
        onTriggered: if (root.flushEditor()) root.controller.save_all_documents()
    }
    Kirigami.Action {
        id: saveAction
        text: "Save"
        icon.name: "document-save"
        enabled: root.controller.active_document_id.length > 0
            && (root.controller.active_document_dirty || root.controller.preview_document_id === root.controller.active_document_id)
            && !root.controller.active_document_external_change
        onTriggered: root.saveActive()
    }
    Kirigami.Action {
        id: newFileAction
        text: "New File…"
        icon.name: "document-new"
        onTriggered: { entryDialogs.create(false) }
    }
    Kirigami.Action {
        id: newFolderAction
        text: "New Folder…"
        icon.name: "folder-new"
        onTriggered: { entryDialogs.create(true) }
    }
    Kirigami.Action {
        id: developerAction
        text: "Developer Tools"
        icon.name: "tools-report-bug"
        visible: root.controller.devtools_available
        onTriggered: root.controller.open_devtools()
    }
    readonly property bool terminalQuitPending: terminalQuitWait.running
    function requestTerminalQuit() {
        if (!controller.terminals_running) { ApplicationLifecycle.finishQuit(); return }
        terminalQuitDialog.open()
    }
    Dialog {
        id: terminalQuitDialog
        width: 420
        parent: root.showingHome ? homeWindow.Overlay.overlay : root.Overlay.overlay
        anchors.centerIn: parent
        modal: true
        readonly property var prompt: root.controller.terminal_confirmation()
        title: prompt.title
        contentItem: Label { text: terminalQuitDialog.prompt.body; wrapMode: Text.WordWrap; width: 360 }
        footer: DialogButtonBox {
            Button { text: terminalQuitDialog.prompt.cancel; DialogButtonBox.buttonRole: DialogButtonBox.RejectRole }
            Button { text: terminalQuitDialog.prompt.confirm; DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole }
            onRejected: terminalQuitDialog.close()
            onAccepted: {
                terminalQuitDialog.close()
                root.controller.terminal_action("close_all",0,true)
                terminalQuitWait.start()
            }
        }
    }
    Timer {
        id: terminalQuitWait
        interval: 16
        repeat: true
        onTriggered: {
            root.controller.poll()
            if (!root.controller.terminals_running) { stop(); ApplicationLifecycle.finishQuit() }
        }
    }
    Kirigami.Action {
        id: quitAction
        text: "Quit"
        icon.name: "application-exit"
        shortcut: StandardKey.Quit
        onTriggered: root.requestQuit()
    }

    WorkspaceHome {
        id: homeWindow
        workspace: root
        drawerActions: [developerAction, quitAction]
        pageActions: [openAction, resumeAction]
        browseAction: openAction
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
                        contentItem: TabBar {
                            id: sidebarTabs
                            objectName: "sidebarTabs"
                            background: null
                            bottomPadding: 0
                            implicitHeight: root.documentTabHeight
                            contentHeight: availableHeight
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
                                    height: sidebarTabs.availableHeight
                                    text: modelData.title
                                    icon.name: modelData.icon
                                    icon.width: Kirigami.Units.iconSizes.smallMedium
                                    icon.height: Kirigami.Units.iconSizes.smallMedium
                                    display: AbstractButton.IconOnly
                                    checked: root.sidebarTab === index
                                    ToolTip.visible: hovered
                                    ToolTip.text: text
                                    ToolTip.delay: Kirigami.Units.toolTipDelay
                                }
                            }
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
                                RowLayout {
                                    id: fileSearchRow
                                    Layout.fillWidth: true
                                    Layout.minimumHeight: root.documentTabHeight
                                    Layout.preferredHeight: root.documentTabHeight
                                    Layout.maximumHeight: root.documentTabHeight
                                    Layout.leftMargin: Kirigami.Units.smallSpacing
                                    Layout.rightMargin: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === WorkspacePane.Files
                                    ToolButton {
                                        id: addEntryButton
                                        objectName: "addEntryButton"
                                        Layout.fillHeight: true
                                        Layout.preferredWidth: root.documentTabHeight
                                        text: "Add file or folder"
                                        icon.name: "list-add"
                                        display: AbstractButton.IconOnly
                                        ToolTip.visible: hovered
                                        ToolTip.text: text
                                        ToolTip.delay: Kirigami.Units.toolTipDelay
                                        onClicked: createEntryMenu.open()
                                        Menu {
                                            id: createEntryMenu
                                            objectName: "createEntryMenu"
                                            y: addEntryButton.height
                                            MenuItem { action: newFileAction }
                                            MenuItem { action: newFolderAction }
                                        }
                                    }
                                    Kirigami.SearchField {
                                        id: fileNameFilter
                                        objectName: "fileNameFilter"
                                        Layout.fillHeight: true
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
                                    id: textSearchRow
                                    Layout.fillWidth: true
                                    Layout.minimumHeight: root.documentTabHeight
                                    Layout.preferredHeight: root.documentTabHeight
                                    Layout.maximumHeight: root.documentTabHeight
                                    Layout.leftMargin: Kirigami.Units.smallSpacing
                                    Layout.rightMargin: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === WorkspacePane.Search
                                    Kirigami.SearchField {
                                        id: textSearchField
                                        Layout.fillWidth: true
                                        Layout.fillHeight: true
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
                                SearchPanel {
                                    id: searchControls
                                    workspace: root
                                    visible: root.sidebarTab === WorkspacePane.Search
                                    Layout.fillWidth: true
                                    Layout.leftMargin: Kirigami.Units.smallSpacing
                                    Layout.rightMargin: Kirigami.Units.smallSpacing
                                }
                                Label {
                                    objectName: "searchTruncated"
                                    Layout.fillWidth: true
                                    Layout.leftMargin: Kirigami.Units.smallSpacing
                                    Layout.rightMargin: Kirigami.Units.smallSpacing
                                    visible: root.sidebarTab === WorkspacePane.Search && root.controller.search_truncated
                                    text: "Results are incomplete. Refresh or refine the search before replacing."
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
                                visible: root.sidebarTab > WorkspacePane.Git
                                Kirigami.PlaceholderMessage {
                                    anchors.centerIn: parent
                                    width: parent.width - Kirigami.Units.largeSpacing * 2
                                    text: root.sidebarSections[root.sidebarTab]?.title ?? ""
                                    icon.name: root.sidebarSections[root.sidebarTab]?.icon ?? ""
                                    explanation: root.sidebarSections[root.sidebarTab]?.explanation ?? ""
                                }
                            }
                            GitPanel {
                                objectName: "gitPanel"
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: Kirigami.Units.smallSpacing
                                visible: root.sidebarTab === WorkspacePane.Git
                                controller: root.controller
                            }
                            TreeView {
                                id: fileTree
                                objectName: "fileTree"
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: workspaceDropTarget.top
                                anchors.bottomMargin: Kirigami.Units.smallSpacing
                                visible: root.sidebarTab === WorkspacePane.Files
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
                                    if (root.sidebarTab !== WorkspacePane.Files || root.restoringFileTree || root.fileFilter.length > 0) return
                                    var entry = root.treeEntry(fileTree.index(row, 0))
                                    if (entry && root.collapsedDirectories[entry.path] !== false)
                                        root.toggleDirectory(entry.path)
                                }
                                onCollapsed: function(row) {
                                    if (root.sidebarTab !== WorkspacePane.Files || root.restoringFileTree || root.fileFilter.length > 0) return
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
                                    property string gitStatus: entry ? root.gitStatus(entry.path) : ""
                                    text: model.display
                                    icon.source: entry ? root.entryIcon(entry.path, entry.directory, expanded) : ""
                                    icon.color: "transparent"
                                    icon.width: Kirigami.Units.iconSizes.small
                                    icon.height: Kirigami.Units.iconSizes.small
                                    rightPadding: Math.max(Kirigami.Units.mediumSpacing, treeStatus.implicitWidth + Kirigami.Units.smallSpacing)
                                    Row {
                                        id: treeStatus
                                        anchors.right: parent.right
                                        anchors.rightMargin: Kirigami.Units.smallSpacing
                                        anchors.verticalCenter: parent.verticalCenter
                                        spacing: Kirigami.Units.smallSpacing
                                        visible: !treeRow.editing && !treeRow.pooled
                                        Kirigami.Icon {
                                            objectName: "sidebarOpen:" + (treeRow.entry ? treeRow.entry.path : "")
                                            source: "view-visible"
                                            visible: treeRow.entry && !treeRow.entry.directory && root.controller.open_document_ids.indexOf(treeRow.entry.path) !== -1
                                            width: Kirigami.Units.iconSizes.small
                                            height: width
                                            Accessible.name: "Open in editor"
                                        }
                                        Kirigami.Icon {
                                            source: "dialog-warning"
                                            visible: treeRow.entry && !treeRow.entry.directory && root.controller.conflicted_document_ids.indexOf(treeRow.entry.path) !== -1
                                            width: Kirigami.Units.iconSizes.small
                                            height: width
                                            Accessible.name: "File changed on disk"
                                        }
                                        Rectangle {
                                            objectName: "sidebarDirty:" + (treeRow.entry ? treeRow.entry.path : "")
                                            visible: treeRow.entry && !treeRow.entry.directory && root.controller.dirty_document_ids.indexOf(treeRow.entry.path) !== -1
                                            anchors.verticalCenter: parent.verticalCenter
                                            width: 6
                                            height: 6
                                            radius: width / 2
                                            color: treeRow.highlighted ? systemColors.highlightedText : treeRow.Kirigami.Theme.textColor
                                            Accessible.name: "Unsaved changes"
                                        }
                                        // Last item: a fixed trailing column like VS Code.
                                        Label {
                                            objectName: "sidebarGit:" + (treeRow.entry ? treeRow.entry.path : "")
                                            text: treeRow.gitStatus
                                            color: root.gitColor(treeRow.gitStatus)
                                            font.bold: true
                                            width: Kirigami.Units.iconSizes.small
                                            horizontalAlignment: Text.AlignHCenter
                                            visible: text.length > 0
                                            Accessible.name: "Git status: " + text
                                        }
                                    }
                                    contentItem: Kirigami.IconTitleSubtitle {
                                        title: treeRow.text
                                        icon: icon.fromControlsIcon(treeRow.icon)
                                        font: treeRow.font
                                        Kirigami.Theme.textColor: root.gitColor(treeRow.gitStatus)
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
                                        MenuItem {
                                            text: "Delete…"
                                            onTriggered: root.requestDelete(entryMenu.path)
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
                                        keys: ["native-workspace-entry", "text/uri-list"]
                                        property string directory: treeRow.entry ? (treeRow.entry.directory
                                            ? treeRow.entry.path : root.parentDirectory(treeRow.entry.path)) : ""
                                        onEntered: function(drag) {
                                            if (drag.source === dragGhost) root.hoverDrop(directory)
                                            else root.enterExternalDrop(drag, directory)
                                        }
                                        onPositionChanged: function(drag) {
                                            if (drag.source === dragGhost) root.hoverDrop(directory)
                                            else root.enterExternalDrop(drag, directory)
                                        }
                                        onExited: root.leaveDrop(directory)
                                        onDropped: function(drop) {
                                            if (drop.source === dragGhost && root.dropEntry(directory)) drop.accept(Qt.MoveAction)
                                            else if (drop.hasUrls) root.dropExternalFiles(drop, directory)
                                        }
                                    }
                                    onDoubleClicked: if (entry && !entry.directory && root.flushEditor())
                                        root.controller.open_document(entry.path)
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
                                visible: root.sidebarTab === WorkspacePane.Files && (root.draggingPath.length > 0 || root.externalDropUrls.length > 0)
                                text: root.hoveringDrop && !root.validDrop ? "This folder cannot receive the item" : "Move to workspace root"
                                contentItem: Kirigami.IconTitleSubtitle {
                                    title: workspaceDropTarget.text
                                    icon.name: root.hoveringDrop && !root.validDrop ? "dialog-warning" : "go-up"
                                }
                                DropArea {
                                    anchors.fill: parent
                                    keys: ["native-workspace-entry", "text/uri-list"]
                                    onEntered: function(drag) {
                                        if (drag.source === dragGhost) root.hoverDrop("")
                                        else root.enterExternalDrop(drag, "")
                                    }
                                    onExited: root.leaveDrop("")
                                    onDropped: function(drop) {
                                        if (drop.source === dragGhost && root.dropEntry("")) drop.accept(Qt.MoveAction)
                                        else if (drop.hasUrls) root.dropExternalFiles(drop, "")
                                    }
                                }
                            }
                            DropArea {
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                z: -1
                                enabled: root.sidebarTab === WorkspacePane.Files
                                keys: ["text/uri-list"]
                                onEntered: function(drag) { root.enterExternalDrop(drag, "") }
                                onPositionChanged: function(drag) { root.enterExternalDrop(drag, "") }
                                onExited: root.leaveDrop("")
                                onDropped: function(drop) { root.dropExternalFiles(drop, "") }
                            }
                            ListView {
                                id: fileList
                                objectName: "searchResults"
                                anchors.top: sidebarHeader.bottom
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                visible: root.sidebarTab === WorkspacePane.Search
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
                                        onDoubleClicked: if (!fileGroup.isDirectory && root.flushEditor())
                                            root.controller.open_document(fileGroup.fileId)
                                        Row {
                                            id: searchStatus
                                            anchors.right: parent.right
                                            anchors.rightMargin: Kirigami.Units.smallSpacing
                                            anchors.verticalCenter: parent.verticalCenter
                                            spacing: Kirigami.Units.smallSpacing
                                            Kirigami.Icon {
                                                objectName: "sidebarSearchOpen:" + fileGroup.fileId
                                                source: "view-visible"
                                                visible: !fileGroup.isDirectory && root.controller.open_document_ids.indexOf(fileGroup.fileId) !== -1
                                                width: Kirigami.Units.iconSizes.small
                                                height: width
                                                Accessible.name: "Open in editor"
                                            }
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
                                                text: "Replace All in File…"
                                                enabled: !fileGroup.isDirectory && !root.controller.search_in_progress && !root.controller.search_truncated
                                                onTriggered: searchControls.replace(fileGroup.fileId, -1)
                                            }
                                            MenuItem {
                                                text: "Rename…"
                                                icon.name: "edit-rename"
                                                onTriggered: root.beginRename(fileGroup.fileId)
                                            }
                                            MenuItem {
                                                text: "Delete…"
                                                onTriggered: root.requestDelete(fileGroup.fileId)
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
                                            text: searchResults.get(resultIndex).snippet || ""
                                            subtitle: "Line " + searchResults.get(resultIndex).line + ", Column " + searchResults.get(resultIndex).column
                                            icon.name: "go-jump"
                                            highlighted: root.selectedResultIndex === resultIndex
                                            onClicked: root.openSearchResult(resultIndex, fileGroup.fileId)
                                            onDoubleClicked: if (root.flushEditor()) root.controller.keep_document_open(fileGroup.fileId)
                                            TapHandler {
                                                acceptedButtons: Qt.RightButton
                                                onTapped: matchReplaceMenu.popup()
                                            }
                                            Menu {
                                                id: matchReplaceMenu
                                                MenuItem {
                                                    text: "Replace…"
                                                    onTriggered: searchControls.replace("", resultIndex)
                                                }
                                                MenuItem {
                                                    text: "Replace All in File…"
                                                    onTriggered: searchControls.replace(fileGroup.fileId, -1)
                                                }
                                            }
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
                DropArea {
                    objectName: "externalEditorDrop"
                    anchors.fill: parent
                    z: 10
                    keys: ["text/uri-list"]
                    onDropped: function(drop) {
                        if (!drop.hasUrls || !root.flushEditor()) return
                        root.controller.open_files(drop.urls.map(function(url) { return url.toString() }))
                        drop.accept(Qt.CopyAction)
                    }
                }
                ColumnLayout {
                    anchors.fill: parent
                    spacing: Kirigami.Units.smallSpacing
                    RowLayout {
                        Layout.fillWidth: true
                        TabBar {
                            id: documentTabBar
                            objectName: "documentTabBar"
                            onCurrentIndexChanged: {
                                if (currentItem && !root.synchronizingDocumentTabs && !root.controller.snapshot_updating)
                                    root.selectDocumentTab(currentItem.path)
                            }
                            implicitHeight: root.documentTabHeight
                            contentHeight: availableHeight
                            Layout.fillWidth: true
                            Layout.minimumHeight: root.documentTabHeight
                            Layout.preferredHeight: root.documentTabHeight
                            Layout.maximumHeight: root.documentTabHeight
                            Repeater {
                                model: documentTabsModel
                                delegate: TabButton {
                                    required property string path
                                    objectName: "documentTab:" + path
                                    width: implicitWidth
                                    height: documentTabBar.availableHeight
                                    icon.width: Kirigami.Units.iconSizes.smallMedium
                                    icon.height: Kirigami.Units.iconSizes.smallMedium
                                    icon.source: root.entryIcon(path, false, false)
                                    icon.color: "transparent"
                                    text: path.split("/").pop()
                                    font.italic: root.controller.preview_document_id === path
                                    onDoubleClicked: if (root.flushEditor()) root.controller.keep_document_open(path)
                                    TapHandler {
                                        acceptedButtons: Qt.RightButton
                                        onTapped: documentTabMenu.popup()
                                    }
                                    Menu {
                                        id: documentTabMenu
                                        MenuItem {
                                            text: "Keep Open"
                                            icon.name: "window-pin"
                                            enabled: root.controller.preview_document_id === path
                                            onTriggered: if (root.flushEditor()) root.controller.keep_document_open(path)
                                        }
                                    }
                                    Accessible.description: root.controller.dirty_document_ids.indexOf(path) !== -1 ? "Unsaved changes" : ""
                                    leftPadding: documentDirtyIndicator.width + Kirigami.Units.smallSpacing * 2
                                    rightPadding: closeDocumentTab.width + Kirigami.Units.smallSpacing
                                    ToolTip.text: path
                                    ToolTip.visible: hovered
                                    Rectangle {
                                        id: documentDirtyIndicator
                                        objectName: "dirtyDocumentTab:" + path
                                        anchors.left: parent.left
                                        anchors.leftMargin: Kirigami.Units.smallSpacing
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: 5
                                        height: 5
                                        radius: width / 2
                                        color: Kirigami.Theme.textColor
                                        visible: root.controller.dirty_document_ids.indexOf(path) !== -1
                                    }
                                    ToolButton {
                                        id: closeDocumentTab
                                        objectName: "closeDocumentTab:" + path
                                        anchors.right: parent.right
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: height
                                        height: parent.availableHeight
                                        padding: Math.min(Kirigami.Units.smallSpacing, height / 4)
                                        icon.width: Kirigami.Units.iconSizes.small
                                        icon.height: Kirigami.Units.iconSizes.small
                                        icon.name: "window-close"
                                        display: AbstractButton.IconOnly
                                        text: "Close " + path
                                        Accessible.name: text
                                        onClicked: root.requestCloseDocument(path)
                                    }
                                }
                            }
                        }
                    }
                    Pane {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        padding: 0
                        Kirigami.Theme.colorSet: Kirigami.Theme.View
                        RowLayout {
                            anchors.fill: parent
                            spacing: 0
                            visible: root.controller.active_document_id.length > 0
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
                                    font: Qt.font({ family: EditorFonts.fixedFont.family, pointSize: root.editorFontPointSize })
                                    textFormat: TextEdit.PlainText
                                    wrapMode: TextEdit.NoWrap
                                    EditorSyntaxHighlighter {
                                        id: syntaxHighlighter
                                        textDocument: editor.textDocument
                                        colors: root.syntaxColors
                                    }
                                    enabled: root.controller.active_document_id.length > 0 && !root.controller.editing_paused
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
                        ScrollView {
                            id: hubScroll
                            objectName: "workspaceHub"
                            implicitWidth: 0
                            implicitHeight: 0
                            anchors.fill: parent
                            visible: root.controller.active_document_id.length === 0
                            contentWidth: availableWidth
                            contentHeight: Math.max(availableHeight, hubContent.implicitHeight)
                            clip: true
                            ColumnLayout {
                                id: hubContent
                                width: Math.min(440, hubScroll.availableWidth)
                                x: (hubScroll.availableWidth - width) / 2
                                y: Math.max(0, (hubScroll.availableHeight - height) / 2)
                                spacing: Kirigami.Units.largeSpacing * 2
                                Kirigami.Icon {
                                    source: "document-edit"
                                    Layout.preferredWidth: Kirigami.Units.iconSizes.large
                                    Layout.preferredHeight: Kirigami.Units.iconSizes.large
                                }
                                Kirigami.Heading {
                                    text: "No Document Open"
                                    wrapMode: Text.WordWrap
                                    Layout.fillWidth: true
                                }
                                Label {
                                    text: "Choose a file from the sidebar to start editing."
                                    opacity: 0.7
                                    wrapMode: Text.WordWrap
                                    Layout.fillWidth: true
                                }
                                Button { action: openAction }
                                Kirigami.Separator { Layout.fillWidth: true }
                                Kirigami.Heading { text: "Keyboard Shortcuts"; level: 3 }
                                GridLayout {
                                    columns: 2
                                    columnSpacing: Kirigami.Units.largeSpacing * 3
                                    rowSpacing: Kirigami.Units.largeSpacing
                                    Repeater {
                                        model: ["Open Folder", openFolderShortcut.nativeText, "Save", saveShortcut.nativeText, "Save All", saveAllShortcut.nativeText]
                                        Label {
                                            required property int index
                                            required property string modelData
                                            text: modelData
                                            opacity: index % 2 === 0 ? 0.7 : 1
                                            Layout.fillWidth: index % 2 === 0
                                            horizontalAlignment: index % 2 === 0 ? Text.AlignLeft : Text.AlignRight
                                        }
                                    }
                                }
                            }
                            padding: Kirigami.Units.largeSpacing * 3
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
                visible: root.controller.editing_paused || root.controller.pending_move_target.length > 0
                running: visible
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                Accessible.name: root.controller.editing_paused ? "Working…" : "Moving… Waiting for confirmation"
            }
            Label {
                text: root.controller.active_document_id || "No document"
                elide: Text.ElideMiddle
                Layout.fillWidth: true
            }
            Label {
                text: root.controller.editing_paused ? "Working…" : (root.controller.active_document_id.length === 0 ? "Ready" : (root.controller.active_document_dirty ? "Modified" : "Saved"))
            }
            ToolButton {
                objectName: "toggleTerminal"
                action: terminalAction
                display: AbstractButton.IconOnly
                checked: root.terminalVisible
                ToolTip.text: "Show or Hide Terminal"
                ToolTip.visible: hovered
                ToolTip.delay: Kirigami.Units.toolTipDelay
            }
        }
    }

    Connections {
        target: root.controller
        function onSnapshot_updatingChanged() {
            if (root.controller.snapshot_updating) root.synchronizingDocumentTabs = true
            else root.syncDocumentTabs()
        }
        function onOpen_document_idsChanged() {
            if (!root.controller.snapshot_updating) root.syncDocumentTabs()
        }
        function onDocument_switched() {
            syntaxHighlighter.clearUndoHistory()
            editor.deselect()
        }
        function onActive_document_idChanged() {
            if (!root.controller.snapshot_updating) root.syncDocumentTabs()
            root.selectedResultIndex = -1
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
