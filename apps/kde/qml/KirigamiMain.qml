import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic

Kirigami.ApplicationWindow {
    visible: true
    width: 900
    height: 600
    title: "Native Workspace"

    AppController { id: controller }
    FolderDialog {
        id: folderDialog
        onAccepted: controller.open_workspace(selectedFolder.toString())
    }
    pageStack.initialPage: Kirigami.Page {
        WorkspacePane {
            anchors.fill: parent
            controller: controller
            enableBrowse: true
            onBrowseRequested: folderDialog.open()
        }
    }

    Component.onCompleted: {
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            controller.increment()
            if (controller.counter !== 1 || controller.revision !== 1)
                throw new Error("Kirigami diagnostic state did not update")
            console.log("Kirigami → CXX-Qt → Rust smoke test passed")
        }
        for (var i = 0; i < Qt.application.arguments.length; ++i) {
            var argument = Qt.application.arguments[i]
            if (argument.startsWith("--smoke-workspace=")) {
                controller.open_workspace(argument.substring("--smoke-workspace=".length))
                if (controller.document_ids.length === 0)
                    throw new Error("Workspace documents were not listed")
                controller.open_document(controller.document_ids[0])
                controller.change_document("Kirigami Phase 1 smoke content")
                if (!controller.active_document_dirty)
                    throw new Error("Document did not become dirty")
                controller.save_document()
                if (controller.active_document_dirty)
                    throw new Error("Document remained dirty after save")
                console.log("Kirigami workspace edit/save smoke test passed")
            }
        }
    }
}
