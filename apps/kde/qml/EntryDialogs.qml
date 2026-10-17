import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Item {
    id: dialogs
    required property var workspace
    readonly property var controller: workspace.controller
    function create(directory) { workspace.creatingDirectory = directory; createEntryDialog.open() }
    Kirigami.Dialog {
        parent: dialogs.workspace.Overlay.overlay
        id: deleteEntryDialog
        objectName: "deleteEntryDialog"
        property string path: ""
        property string workspaceRoot: ""
        modal: true
        title: controller.confirmation(path, true).title
        preferredWidth: Kirigami.Units.gridUnit * 26
        standardButtons: Kirigami.Dialog.NoButton
        Label {
            text: controller.confirmation(deleteEntryDialog.path, true).body
            wrapMode: Text.WordWrap
        }
        customFooterActions: [
            Kirigami.Action {
                text: "Cancel"
                onTriggered: deleteEntryDialog.close()
            },
            Kirigami.Action {
                text: controller.confirmation(deleteEntryDialog.path, true).confirm
                icon.name: "edit-delete"
                onTriggered: {
                    deleteEntryDialog.close()
                    if (deleteEntryDialog.workspaceRoot === dialogs.workspace.controller.workspace_root && dialogs.workspace.flushEditor())
                        dialogs.workspace.controller.delete_entry(deleteEntryDialog.path)
                }
            }
        ]
    }

    function requestCloseDocument(path) {
        if (!dialogs.workspace.flushEditor()) return
        if (dialogs.workspace.controller.dirty_document_ids.indexOf(path) === -1) {
            dialogs.workspace.controller.close_document(path)
        } else {
            closeDocumentDialog.path = path
            closeDocumentDialog.workspaceRoot = dialogs.workspace.controller.workspace_root
            closeDocumentDialog.open()
        }
    }
    Kirigami.Dialog {
        parent: dialogs.workspace.Overlay.overlay
        id: closeDocumentDialog
        objectName: "closeDocumentDialog"
        property string path: ""
        property string workspaceRoot: ""
        title: controller.confirmation(path, false).title
        standardButtons: Kirigami.Dialog.NoButton
        Label { text: controller.confirmation(closeDocumentDialog.path, false).body; wrapMode: Text.WordWrap }
        customFooterActions: [
            Kirigami.Action { text: "Cancel"; onTriggered: closeDocumentDialog.close() },
            Kirigami.Action { text: controller.confirmation(closeDocumentDialog.path, false).discard; onTriggered: {
                closeDocumentDialog.close()
                if (closeDocumentDialog.workspaceRoot === dialogs.workspace.controller.workspace_root && dialogs.workspace.flushEditor())
                    dialogs.workspace.controller.discard_and_close_document(closeDocumentDialog.path)
            } },
            Kirigami.Action { text: controller.confirmation(closeDocumentDialog.path, false).confirm; onTriggered: {
                closeDocumentDialog.close()
                if (closeDocumentDialog.workspaceRoot === dialogs.workspace.controller.workspace_root && dialogs.workspace.flushEditor())
                    dialogs.workspace.controller.save_and_close_document(closeDocumentDialog.path)
            } }
        ]
    }

    function requestDelete(path) {
        if (dialogs.workspace.controller.move_pending) return
        deleteEntryDialog.workspaceRoot = dialogs.workspace.controller.workspace_root
        deleteEntryDialog.path = path
        deleteEntryDialog.open()
    }

    Kirigami.Dialog {
        parent: dialogs.workspace.Overlay.overlay
        id: createEntryDialog
        preferredWidth: Kirigami.Units.gridUnit * 24
        title: dialogs.workspace.creatingDirectory ? "New Folder" : "New File"
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        Kirigami.FormLayout {
            TextField {
                id: newEntryPath
                Kirigami.FormData.label: "Relative path:"
                placeholderText: dialogs.workspace.creatingDirectory ? "folder" : "file.txt"
                onAccepted: createEntryDialog.accept()
            }
        }
        onOpened: {
            newEntryPath.text = ""
            newEntryPath.forceActiveFocus()
        }
        onAccepted: dialogs.workspace.createEntry(newEntryPath.text.trim(), dialogs.workspace.creatingDirectory)
    }

}
