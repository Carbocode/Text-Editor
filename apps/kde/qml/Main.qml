import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic

Kirigami.ApplicationWindow {
    visible: !workspacePane.showingHome
    width: Kirigami.Units.gridUnit * 60
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 36
    minimumHeight: Kirigami.Units.gridUnit * 24
    title: controller.workspace_name.length > 0 ? controller.workspace_name : "Native Workspace"

    globalDrawer: Kirigami.GlobalDrawer {
        title: "Native Workspace"
        titleIcon: "accessories-text-editor"
        isMenu: true
        actions: workspacePane.applicationActions
    }
    AppController { id: controller }
    FolderDialog {
        id: folderDialog
        parentWindow: workspacePane.showingHome ? workspacePane.homeWindow : workspacePane.Window.window
        onAccepted: workspacePane.openWorkspace(selectedFolder.toString())
    }
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None
    pageStack.initialPage: Kirigami.Page {
        title: controller.workspace_name
        padding: 0
        WorkspacePane {
            id: workspacePane
            anchors.fill: parent
            controller: controller
            enableBrowse: true
            onBrowseRequested: folderDialog.open()
        }
    }

    Component.onCompleted: controller.initialize_persistence()
}
