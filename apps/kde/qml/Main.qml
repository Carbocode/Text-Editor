import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic
import org.nativeworkspace.Desktop

Kirigami.ApplicationWindow {
    id: window
    visible: !workspacePane.showingHome
    width: Kirigami.Units.gridUnit * 72
    height: Kirigami.Units.gridUnit * 46
    minimumWidth: Kirigami.Units.gridUnit * 36
    minimumHeight: Kirigami.Units.gridUnit * 24
    onClosing: function(close) { close.accepted = false; workspacePane.requestQuit() }
    title: controller.workspace_name.length > 0 ? controller.workspace_name : "Native Workspace"

    globalDrawer: Kirigami.GlobalDrawer {
        title: "Native Workspace"
        titleIcon: "accessories-text-editor"
        isMenu: true
        actions: workspacePane.applicationActions
    }
    AppController { id: controller; objectName: "workspaceController" }
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
            objectName: "workspacePane"
            anchors.fill: parent
            controller: controller
            enableBrowse: true
            onBrowseRequested: folderDialog.open()
        }
    }

    function openSystemRequests() {
        if (!SystemOpenRequests.hasPending() || !workspacePane.flushEditor()) return
        const urls = SystemOpenRequests.takePending()
        controller.open_paths(urls)
        if (controller.workspace_root.length > 0) {
            workspacePane.showingHome = false
            window.show()
            window.raise()
            window.requestActivate()
        }
    }
    Connections {
        target: SystemOpenRequests
        function onReceived() { window.openSystemRequests() }
    }
    Connections {
        target: controller
        function onMove_pendingChanged() {
            if (!controller.move_pending) Qt.callLater(function() { window.openSystemRequests() })
        }
    }
    Component.onCompleted: {
        ApplicationLifecycle.watchQuitRequests()
        const persistenceReady = controller.initialize_persistence()
        if (SystemOpenRequests.hasPending()) openSystemRequests()
        else if (persistenceReady) controller.restore_session()
    }
}
