import QtQuick
import org.nativeworkspace.Desktop
import "SmokeLifecycle.js" as SmokeLifecycle
import QtQuick.Controls
import QtTest
import org.nativeworkspace.Diagnostic
import org.nativeworkspace.Test
import "../qml"

ApplicationWindow {
    id: window
    width: 1000
    height: 650
    visible: true
    AppController { id: controller }
    WorkspacePane { id: pane; anchors.fill: parent; controller: controller }
    TestCase { id: input; when: false }
    property string source: Qt.application.arguments.find(value => value.startsWith("--smoke-external-source=")).substring("--smoke-external-source=".length)
    property var point
    property int step: 0
    property double started: Date.now()
    function find(item, name) {
        if (item.objectName === name) return item
        for (var child of item.children || []) { var found = find(child, name); if (found) return found }
        return null
    }
    function check(value, message) { if (!value) throw new Error(message) }
    function center(item) {
        check(item && item.width > 0 && item.height > 0, "Missing file drop target")
        return item.mapToItem(window.contentItem, item.width / 2, item.height / 2)
    }
    Component.onCompleted: {
        controller.open_workspace(Qt.application.arguments.find(value => value.startsWith("--smoke-external=")).substring("--smoke-external=".length))
        pane.showingHome = false
    }
    Timer {
        running: true
        repeat: true
        interval: 150
        onTriggered: {
            try {
                switch (step++) {
                case 0:
                    point = center(find(pane, "externalEditorDrop"))
                    check(FileDropProbe.enter(window, point.x, point.y, [source]), "Editor refused file URLs")
                    check(FileDropProbe.drop(window, point.x, point.y), "Editor drop was not accepted")
                    break
                case 1:
                    if (controller.active_document_id.length === 0) { --step; break }
                    check(controller.active_document_id.startsWith("/"), "External file did not open")
                    check(controller.preview_document_id.length === 0, "External drop created a preview")
                    check(!pane.sidebarEntries().some(entry => entry.path.startsWith("/")), "External tab leaked into the tree")
                    find(pane, "documentEditor").text = "external edited"
                    pane.flushEditor()
                    var tree = find(pane, "fileTree")
                    tree.forceLayout()
                    point = center(tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src"))))
                    check(FileDropProbe.enter(window, point.x, point.y, [source]), "Folder refused file URLs")
                    break
                case 2:
                case 3:
                case 4:
                    FileDropProbe.move(window, point.x, point.y)
                    break
                case 5:
                    check(pane.collapsedDirectories["src"] === false, "External hover did not expand the folder")
                    FileDropProbe.leave(window)
                    break
                case 6:
                    check(pane.collapsedDirectories["src"] !== false, "Cancelled external hover left the folder expanded")
                    var tree = find(pane, "fileTree")
                    tree.forceLayout()
                    point = center(tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src"))))
                    check(FileDropProbe.enter(window, point.x, point.y, [source]), "Folder refused a second file drag")
                    break
                case 7:
                case 8:
                case 9:
                    FileDropProbe.move(window, point.x, point.y)
                    break
                case 10:
                    check(FileDropProbe.drop(window, point.x, point.y), "Folder drop was not accepted")
                    break
                case 11:
                    if (controller.editing_paused) { --step; break }
                    check(controller.active_document_id === "src/" + source.split("/").pop(), "External tab did not follow the move")
                    check(controller.active_document_text === "external edited" && controller.active_document_dirty, "Move lost unsaved edits")
                    controller.save_all_documents()
                    check(!controller.active_document_dirty, "Moved external buffer did not save")
                    var rootSource = controller.workspace_root + "/" + controller.active_document_id
                    var tree = find(pane, "fileTree")
                    point = tree.mapToItem(window.contentItem, tree.width / 2, tree.height - 70)
                    check(FileDropProbe.enter(window, point.x, point.y, [rootSource]), "Tree background refused a file drag")
                    check(FileDropProbe.drop(window, point.x, point.y), "Tree background drop was not accepted")
                    break
                case 12:
                    if (controller.editing_paused) { --step; break }
                    check(controller.active_document_id === source.split("/").pop(), "Tree background did not move into the workspace root")
                    console.log("Qt native URI-list editor/folder/root drop/hover/cancel/dirty-buffer smoke test passed")
                    SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
                    break
                }
                if (Date.now() - started > 10000) throw new Error("External drop smoke timed out")
            } catch (error) { console.error(error); SmokeLifecycle.quit(controller, ApplicationLifecycle, 1) }
        }
    }
}
