import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    property var controller
    property bool enableBrowse: false
    property bool syncingEditor: false
    property int selectedResultIndex: -1
    property alias editorText: editor.text
    property alias editorSelectedText: editor.selectedText
    property real editorScrollY: editorScroll.contentItem.contentY
    property int searchFileCount: fileIds().length
    property var collapsedDirectories: ({})
    property string lastWorkspaceRoot: ""
    signal browseRequested()

    Connections {
        target: root.controller
        function onWorkspace_rootChanged() {
            if (root.lastWorkspaceRoot !== root.controller.workspace_root) {
                root.lastWorkspaceRoot = root.controller.workspace_root
                root.collapsedDirectories = ({})
            }
        }
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

    function sidebarEntries() {
        var files = fileIds()
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
        }
        function visible(path) {
            if (root.controller.search_query.length > 0) return true
            var parts = path.split("/")
            for (var i = 1; i < parts.length; ++i) {
                if (root.collapsedDirectories[parts.slice(0, i).join("/")] !== false) return false
            }
            return true
        }
        var paths = []
        for (var d = 0; d < directories.length; ++d)
            if (visible(directories[d])) paths.push({ path: directories[d], directory: true })
        for (var f = 0; f < files.length; ++f)
            if (visible(files[f])) paths.push({ path: files[f], directory: false })
        paths.sort(function(a, b) {
            var aParts = a.path.split("/")
            var bParts = b.path.split("/")
            var aPrefix = ""
            var bPrefix = ""
            for (var i = 0; i < Math.min(aParts.length, bParts.length); ++i) {
                aPrefix += (i ? "/" : "") + aParts[i]
                bPrefix += (i ? "/" : "") + bParts[i]
                if (aParts[i] === bParts[i]) continue
                var aDirectory = !!seenDirectories["#" + aPrefix]
                var bDirectory = !!seenDirectories["#" + bPrefix]
                if (aDirectory !== bDirectory) return aDirectory ? -1 : 1
                return aParts[i].localeCompare(bParts[i])
            }
            return aParts.length - bParts.length
        })
        return paths
    }

    function toggleDirectory(path) {
        var next = ({})
        for (var key in collapsedDirectories) next[key] = collapsedDirectories[key]
        next[path] = !(collapsedDirectories[path] !== false)
        collapsedDirectories = next
    }

    function matchIndicesForFile(id) {
        var indices = []
        for (var i = 0; i < controller.search_result_ids.length; ++i) {
            if (controller.search_result_ids[i] === id && controller.search_result_lines[i] !== "")
                indices.push(i)
        }
        return indices
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
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: root.sidebarEntries()
                        Label {
                            anchors.centerIn: parent
                            text: root.controller.search_query.length > 0 ? "No matching files" : "No files"
                            visible: root.controller.workspace_name.length > 0 && root.sidebarEntries().length === 0 && !root.controller.search_in_progress
                        }
                        delegate: Column {
                            id: fileGroup
                            width: ListView.view.width
                            property string fileId: modelData.path
                            property bool isDirectory: modelData.directory
                            property int depth: fileId.split("/").length - 1
                            ItemDelegate {
                                width: parent.width
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
                                        sourceSize.width: 64
                                        sourceSize.height: 64
                                        fillMode: Image.PreserveAspectFit
                                        mipmap: true
                                        Layout.preferredWidth: 20
                                        Layout.preferredHeight: 20
                                    }
                                    Label {
                                        visible: fileGroup.isDirectory
                                        text: root.controller.search_query.length === 0 && root.collapsedDirectories[fileGroup.fileId] !== false ? "▸" : "▾"
                                    }
                                    Image {
                                        visible: fileGroup.isDirectory
                                        source: root.controller.folder_icon(fileGroup.fileId, root.controller.search_query.length > 0 || root.collapsedDirectories[fileGroup.fileId] === false)
                                        sourceSize.width: 64
                                        sourceSize.height: 64
                                        fillMode: Image.PreserveAspectFit
                                        mipmap: true
                                        Layout.preferredWidth: 20
                                        Layout.preferredHeight: 20
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
                                model: fileGroup.isDirectory ? [] : root.matchIndicesForFile(fileGroup.fileId)
                                delegate: ItemDelegate {
                                    property int resultIndex: modelData
                                    width: fileGroup.width
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
            ScrollView {
                id: editorScroll
                SplitView.fillWidth: true
                TextArea {
                    id: editor
                    font.family: "monospace"
                    enabled: root.controller.active_document_id.length > 0
                    placeholderText: "Select a document"
                    onTextChanged: {
                        if (!root.syncingEditor && root.controller.active_document_id.length > 0 && text !== root.controller.active_document_text) {
                            root.selectedResultIndex = -1
                            root.controller.change_document(text)
                        }
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
