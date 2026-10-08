import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    property var controller
    property bool enableBrowse: false
    property bool syncingEditor: false
    property alias editorText: editor.text
    signal browseRequested()

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

    function iconFor(name) {
        var lower = name.toLowerCase()
        var base = "qrc:/qt/qml/org/nativeworkspace/Diagnostic/qml/icons/"
        if (lower === "readme.md") return base + "readme.png"
        if (lower.endsWith(".md")) return base + "markdown.png"
        return base + "document.png"
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
                    enabled: root.controller.active_document_dirty
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
                        onTextEdited: root.controller.search_files(text)
                    }
                    ListView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: root.controller.search_result_ids
                        Label {
                            anchors.centerIn: parent
                            text: root.controller.search_query.length > 0 ? "No matching files" : "No files"
                            visible: root.controller.workspace_name.length > 0 && root.controller.search_result_ids.length === 0
                        }
                        delegate: ItemDelegate {
                            width: ListView.view.width
                            highlighted: root.controller.active_document_id === modelData
                            onClicked: if (root.flushEditor()) root.controller.open_document(modelData)
                            contentItem: RowLayout {
                                Image {
                                    source: root.iconFor(modelData)
                                    sourceSize.width: 20
                                    sourceSize.height: 20
                                    fillMode: Image.PreserveAspectFit
                                    Layout.preferredWidth: 20
                                    Layout.preferredHeight: 20
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0
                                    Label { text: modelData; Layout.fillWidth: true; elide: Text.ElideRight }
                                    Label {
                                        text: root.controller.search_result_snippets[index] || ""
                                        visible: text.length > 0
                                        color: palette.placeholderText
                                        font.pointSize: 9
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                    }
                                }
                                Label {
                                    text: "●"
                                    visible: root.controller.dirty_document_ids.indexOf(modelData) !== -1
                                }
                            }
                        }
                    }
                }
            }
            ScrollView {
                SplitView.fillWidth: true
                TextArea {
                    id: editor
                    font.family: "monospace"
                    enabled: root.controller.active_document_id.length > 0
                    placeholderText: "Select a document"
                    onTextChanged: {
                        if (!root.syncingEditor && root.controller.active_document_id.length > 0 && text !== root.controller.active_document_text)
                            root.controller.change_document(text)
                    }
                }
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
                root.syncingEditor = true
                editor.text = root.controller.active_document_text
                root.syncingEditor = false
            }
        }
    }
}
