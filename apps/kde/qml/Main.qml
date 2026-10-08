import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import org.nativeworkspace.Diagnostic

ApplicationWindow {
    visible: true
    width: 900
    height: 600
    title: controller.workspace_name.length > 0 ? controller.workspace_name : "Native Workspace"

    AppController { id: controller }
    FolderDialog {
        id: folderDialog
        onAccepted: controller.open_workspace(selectedFolder.toString())
    }
    WorkspacePane {
        id: workspacePane
        anchors.fill: parent
        controller: controller
        enableBrowse: true
        onBrowseRequested: folderDialog.open()
    }

    Component.onCompleted: {
        if (Qt.application.arguments.indexOf("--smoke") === -1 && !Qt.application.arguments.some(argument => argument.startsWith("--smoke-workspace=")))
            controller.initialize_persistence()
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            controller.increment()
            if (controller.counter !== 1 || controller.revision !== 1)
                throw new Error("CXX-Qt diagnostic state did not update")
            console.log("QML → CXX-Qt → Rust smoke test passed")
        }
        for (var i = 0; i < Qt.application.arguments.length; ++i) {
            var argument = Qt.application.arguments[i]
            if (argument.startsWith("--smoke-workspace=")) {
                controller.open_workspace(argument.substring("--smoke-workspace=".length))
                if (controller.document_ids.length === 0)
                    throw new Error("Workspace documents were not listed")
                if (title !== controller.workspace_name)
                    throw new Error("Window title did not follow the workspace")
                controller.search_files("unlikely-no-match")
                if (controller.search_result_ids.length !== 0)
                    throw new Error("Filename search did not filter results")
                controller.search_files("")
                controller.open_document(controller.document_ids[0])
                workspacePane.editorText = "QML Phase 1 smoke content\n" + "filler\n".repeat(80) + "💡 Another Phase 1 match"
                if (!controller.active_document_dirty)
                    throw new Error("Document did not become dirty")
                workspacePane.saveActive()
                if (controller.active_document_dirty)
                    throw new Error("Document remained dirty after save")
                controller.search_files("phase 1")
                if (controller.search_result_ids.length !== 2 || controller.search_result_snippets[0] !== "QML Phase 1 smoke content")
                    throw new Error("Content search did not return a matching preview")
                workspacePane.openSearchResult(1, controller.document_ids[0])
                Qt.callLater(function() {
                    if (workspacePane.editorSelectedText !== "Phase 1" || workspacePane.editorScrollY <= 0)
                        throw new Error("Selecting a search occurrence did not select its text")
                    console.log("QML workspace edit/save smoke test passed")
                })
            }
        }
    }
}
