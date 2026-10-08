import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import org.nativeworkspace.Syntax 1.0

Item {
    id: root
    property var controller
    property bool enableBrowse: false
    property bool syncingEditor: false
    property bool updatingHighlight: false
    property var minimapLines: [""]
    property var minimapOffsets: [0]
    property var minimapKinds: []
    property var minimapGlyphs: []
    property string minimapSource: ""
    property var minimapSpans: []
    property int selectedResultIndex: -1
    property alias editorText: editor.text
    property alias editorSelectedText: editor.selectedText
    property real editorScrollY: editorScroll.contentItem.contentY
    property int searchFileCount: fileIds().length
    property var collapsedDirectories: ({})
    property var orderedSidebarEntries: []
    property var visibleSidebarEntries: []
    property var childEntriesByParent: ({})
    property var matchIndicesByFile: ({})
    property bool sidebarEntriesDirty: true
    property string lastWorkspaceRoot: ""
    signal browseRequested()

    ListModel { id: sidebarModel }

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
            }
            root.scheduleSidebarUpdate()
        }
        function onDirectory_idsChanged() { root.scheduleSidebarUpdate() }
        function onSearch_result_idsChanged() { root.scheduleSidebarUpdate() }
        function onSearch_queryChanged() { root.scheduleSidebarUpdate() }
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

    function updateHighlight() {
        if (updatingHighlight || !root.controller) return
        updatingHighlight = true
        var source = editor.text
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

    function openSearchResult(index, id) {
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

    function fileIds() {
        var ids = controller.search_result_ids
        var files = []
        var seen = ({})
        for (var i = 0; i < ids.length; ++i) {
            if (!seen["#" + ids[i]]) {
                files.push(ids[i])
                seen["#" + ids[i]] = true
            }
        }
        return files
    }

    function rebuildOrderedEntries() {
        sidebarEntriesDirty = false
        sidebarUpdate.stop()
        var files = fileIds()
        var matches = ({})
        if (root.controller.search_query.length > 0) {
            var resultIds = root.controller.search_result_ids
            var resultLines = root.controller.search_result_lines
            for (var result = 0; result < resultIds.length; ++result) {
                if (resultLines[result] === "") continue
                var resultKey = "#" + resultIds[result]
                if (!matches[resultKey]) matches[resultKey] = []
                matches[resultKey].push(result)
            }
        }
        matchIndicesByFile = matches
        var directories = []
        var seenDirectories = ({})
        if (root.controller.search_query.length === 0) {
            for (var k = 0; k < root.controller.directory_ids.length; ++k) {
                var directory = root.controller.directory_ids[k]
                directories.push(directory)
                seenDirectories["#" + directory] = true
            }
        }
        if (root.controller.search_query.length > 0) {
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
    }

    function updateVisibleEntries() {
        var searching = root.controller.search_query.length > 0
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
        var previous = visibleSidebarEntries
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
        visibleSidebarEntries = visible
    }

    function sidebarEntries() {
        if (sidebarEntriesDirty) rebuildOrderedEntries()
        return visibleSidebarEntries
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
        if (oldCount > 0) sidebarModel.remove(index + 1, oldCount)
        for (var row = 0; row < children.length; ++row)
            sidebarModel.insert(index + 1 + row, children[row])
        visibleSidebarEntries = visible.slice(0, index + 1).concat(children, visible.slice(oldEnd))
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

    ColumnLayout {
        anchors.fill: parent
        spacing: 8
        ToolBar {
            Layout.fillWidth: true
            contentItem: RowLayout {
                spacing: 8
                ToolButton {
                    text: "Open Workspace"
                    icon.name: "document-open"
                    icon.source: "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/icons/action-open.png"
                    icon.width: 20
                    icon.height: 20
                    display: AbstractButton.TextBesideIcon
                    visible: root.enableBrowse
                    onClicked: if (root.flushEditor()) root.browseRequested()
                }
                ToolButton {
                    id: recentButton
                    text: "Recent"
                    icon.name: "document-open-recent"
                    display: AbstractButton.TextBesideIcon
                    enabled: root.controller.recent_workspace_paths.length > 0
                    onClicked: recentMenu.open()
                    Menu {
                        id: recentMenu
                        y: recentButton.height
                        Instantiator {
                            model: root.controller.recent_workspace_paths
                            delegate: MenuItem {
                                text: root.controller.recent_workspace_names[index]
                                onTriggered: if (root.flushEditor()) root.controller.open_workspace(modelData)
                            }
                            onObjectAdded: (index, object) => recentMenu.insertItem(index, object)
                            onObjectRemoved: (index, object) => recentMenu.removeItem(object)
                        }
                    }
                }
                Item { Layout.fillWidth: true }
                ToolButton {
                    text: "Save"
                    icon.name: "document-save"
                    icon.source: "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/icons/action-save.png"
                    icon.width: 20
                    icon.height: 20
                    display: AbstractButton.TextBesideIcon
                    enabled: root.controller.active_document_dirty && !root.controller.active_document_external_change
                    onClicked: root.saveActive()
                }
            }
        }
        SplitView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Pane {
                SplitView.preferredWidth: 240
                SplitView.minimumWidth: 180
                ColumnLayout {
                    anchors.fill: parent
                    TextField {
                        Layout.fillWidth: true
                        text: root.controller.search_query
                        placeholderText: "Search files"
                        enabled: root.controller.workspace_name.length > 0
                        onTextEdited: {
                            root.selectedResultIndex = -1
                            root.controller.search_files(text)
                        }
                    }
                    BusyIndicator {
                        running: root.controller.search_in_progress
                        visible: running
                        Layout.alignment: Qt.AlignHCenter
                    }
                    ListView {
                        id: fileList
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: sidebarModel
                        reuseItems: true
                        Label {
                            anchors.centerIn: parent
                            text: root.controller.search_query.length > 0 ? "No matching files" : "No files"
                            visible: root.controller.workspace_name.length > 0 && root.visibleSidebarEntries.length === 0 && !root.controller.search_in_progress
                        }
                        delegate: Column {
                            id: fileGroup
                            width: ListView.view.width
                            property string fileId: model.path
                            property bool isDirectory: model.directory
                            property int depth: fileId.split("/").length - 1
                            ItemDelegate {
                                width: parent.width
                                height: 26
                                topPadding: 2
                                bottomPadding: 2
                                leftPadding: 10 + fileGroup.depth * 16
                                highlighted: !fileGroup.isDirectory && root.selectedResultIndex < 0 && root.controller.active_document_id === fileGroup.fileId
                                onClicked: {
                                    if (fileGroup.isDirectory) {
                                        root.toggleDirectory(fileGroup.fileId)
                                        return
                                    }
                                    if (root.flushEditor()) {
                                        root.controller.open_document(fileGroup.fileId)
                                        root.selectedResultIndex = -1
                                    }
                                }
                                contentItem: RowLayout {
                                    Image {
                                        visible: !fileGroup.isDirectory
                                        source: root.controller.file_icon(fileGroup.fileId)
                                        sourceSize.width: 32
                                        sourceSize.height: 32
                                        fillMode: Image.PreserveAspectFit
                                        mipmap: true
                                        Layout.preferredWidth: 16
                                        Layout.preferredHeight: 16
                                    }
                                    Label {
                                        visible: fileGroup.isDirectory
                                        text: root.controller.search_query.length === 0 && root.collapsedDirectories[fileGroup.fileId] !== false ? "▸" : "▾"
                                    }
                                    Image {
                                        visible: fileGroup.isDirectory
                                        source: root.controller.folder_icon(fileGroup.fileId, root.controller.search_query.length > 0 || root.collapsedDirectories[fileGroup.fileId] === false)
                                        sourceSize.width: 32
                                        sourceSize.height: 32
                                        fillMode: Image.PreserveAspectFit
                                        mipmap: true
                                        Layout.preferredWidth: 16
                                        Layout.preferredHeight: 16
                                    }
                                    Label {
                                        text: fileGroup.fileId.split("/").pop()
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                    }
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
                                delegate: ItemDelegate {
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
                }
            }
            RowLayout {
                SplitView.fillWidth: true
                spacing: 0
                FontMetrics { id: editorMetrics; font: editor.font }
                Canvas {
                    id: lineNumbers
                    Layout.preferredWidth: 48
                    Layout.fillHeight: true
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        ctx.fillStyle = "#808080"
                        ctx.font = (editorMetrics.font.pixelSize > 0 ? editorMetrics.font.pixelSize : 12) + "px monospace"
                        ctx.textAlign = "right"
                        var lineHeight = editorMetrics.height
                        var scrollY = editorScroll.contentItem.contentY
                        var first = Math.max(0, Math.floor(scrollY / lineHeight) - 1)
                        var last = Math.min(editor.lineCount, first + Math.ceil(height / lineHeight) + 3)
                        for (var line = first; line < last; ++line)
                            ctx.fillText(String(line + 1), width - 8, editor.topPadding + (line + 1) * lineHeight - scrollY)
                    }
                }
                ScrollView {
                    id: editorScroll
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    TextArea {
                        id: editor
                        font.family: "monospace"
                        wrapMode: TextEdit.NoWrap
                        EditorSyntaxHighlighter {
                            id: syntaxHighlighter
                            textDocument: editor.textDocument
                        }
                        enabled: root.controller.active_document_id.length > 0
                        placeholderText: "Select a document"
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
                    property real usedHeight: Math.min(height, root.minimapLines.length * 2)
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        var lines = root.minimapLines
                        var count = Math.max(1, lines.length)
                        var shortDocument = count * 2 <= height
                        var rows = shortDocument ? count : Math.max(1, Math.floor(height))
                        var palette = ["#9ba4ae", "#aa70d3", "#d88b44", "#5eb569", "#858e97", "#569ae0"]
                        for (var row = 0; row < rows; ++row) {
                            var line = shortDocument ? row : Math.floor(row * (count - 1) / Math.max(1, rows - 1))
                            var offset = root.minimapOffsets[line]
                            var y = shortDocument ? row * 2 : row
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
                                        if (shortDocument) {
                                            if (top > 0) {
                                                ctx.globalAlpha = Math.min(1, top / 128)
                                                ctx.fillRect(x, y, 1, 1)
                                            }
                                            if (bottom > 0) {
                                                ctx.globalAlpha = Math.min(1, bottom / 128)
                                                ctx.fillRect(x, y + 1, 1, 1)
                                            }
                                        } else {
                                            ctx.globalAlpha = Math.min(1, Math.max(top, bottom) / 128)
                                            ctx.fillRect(x, y, 1, 1)
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
                        y: editorScroll.contentItem.contentY / Math.max(1, editorScroll.contentItem.contentHeight) * minimap.usedHeight
                        height: Math.max(8, editorScroll.contentItem.height / Math.max(1, editorScroll.contentItem.contentHeight) * minimap.usedHeight)
                        color: "#80808044"
                    }
                    MouseArea {
                        anchors.fill: parent
                        onPressed: moveTo(mouseY)
                        onPositionChanged: if (pressed) moveTo(mouseY)
                        function moveTo(y) {
                            var flick = editorScroll.contentItem
                            flick.contentY = Math.max(0, Math.min(flick.contentHeight - flick.height,
                                y / Math.max(1, minimap.usedHeight) * flick.contentHeight - flick.height / 2))
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
        function onActive_document_idChanged() { root.updateHighlight() }
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
