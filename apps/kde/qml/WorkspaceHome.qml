import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Kirigami.ApplicationWindow {
        id: homeWindow
        required property var workspace
        required property var drawerActions
        required property var pageActions
        required property var browseAction
        transientParent: null
        title: "Native Workspace"
        width: Kirigami.Units.gridUnit * 36
        height: Kirigami.Units.gridUnit * 32
        minimumWidth: Kirigami.Units.gridUnit * 24
        minimumHeight: Kirigami.Units.gridUnit * 20
        visible: homeWindow.workspace.enableBrowse && homeWindow.workspace.showingHome
        onClosing: Qt.quit()
        globalDrawer: Kirigami.GlobalDrawer {
            title: "Native Workspace"
            titleIcon: "accessories-text-editor"
            isMenu: true
            actions: homeWindow.drawerActions
        }
        pageStack.initialPage: Kirigami.ScrollablePage {
            title: "Recent Workspaces"
            actions: homeWindow.pageActions
            ListView {
                model: homeWindow.workspace.controller.recent_workspace_paths
                delegate: Kirigami.SubtitleDelegate {
                    required property int index
                    required property string modelData
                    width: ListView.view.width
                    text: homeWindow.workspace.controller.recent_workspace_names[index]
                    subtitle: modelData
                    icon.source: homeWindow.workspace.controller.folder_icon(modelData, false)
                    icon.color: "transparent"
                    onClicked: homeWindow.workspace.openWorkspace(modelData)
                }
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.largeSpacing * 2
                    visible: homeWindow.workspace.controller.recent_workspace_paths.length === 0
                    icon.name: "document-open-folder"
                    text: "No Recent Workspaces"
                    explanation: "Open a folder to start editing."
                    helpfulAction: homeWindow.browseAction
                }
            }
            footer: Kirigami.InlineMessage {
                visible: homeWindow.workspace.controller.error_message.length > 0
                text: homeWindow.workspace.controller.error_message
                type: Kirigami.MessageType.Error
            }
        }
    }

