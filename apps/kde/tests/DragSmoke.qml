import QtQuick
import QtQuick.Controls
import QtTest
import org.nativeworkspace.Diagnostic
import "../qml"

ApplicationWindow {
    width: 900
    height: 600
    visible: true
    AppController { id: controller }
    WorkspacePane { id: pane; anchors.fill: parent; controller: controller }
    TestCase { id: input; when: false }

    function findItem(item, name) {
        if (item.objectName === name) return item
        for (var child of item.children || []) {
            var found = findItem(child, name)
            if (found) return found
        }
        return null
    }
    function check(condition, message) { if (!condition) throw new Error(message) }
    Component.onCompleted: {
        var argument = Qt.application.arguments.find(value => value.startsWith("--smoke-drag="))
        controller.open_workspace(argument.substring("--smoke-drag=".length))
        pane.showingHome = false
    }
    property int step: 0
    property var sourcePoint
    property var targetPoint
    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            try {
                switch (step++) {
                case 0:
                    pane.sidebarEntries()
                    var tree = findItem(pane, "fileTree")
                    tree.forceLayout()
                    var source = tree.itemAtCell(Qt.point(0, pane.treeRowForPath("README.md")))
                    var target = tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src")))
                    check(source && target, "Missing drag rows")
                    sourcePoint = source.mapToItem(pane, 100, source.height / 2)
                    targetPoint = target.mapToItem(pane, 100, target.height / 2)
                    input.mousePress(pane, sourcePoint.x, sourcePoint.y)
                    input.mouseMove(pane, sourcePoint.x + 20, sourcePoint.y, 1, Qt.LeftButton)
                    input.mouseMove(pane, targetPoint.x, targetPoint.y, 1, Qt.LeftButton)
                    break
                case 1:
                    check(pane.draggingPath === "README.md", "Pointer did not start drag")
                    check(pane.hoveringDrop && pane.hoverDirectory === "src", "Pointer did not highlight folder")
                    // Motion within the folder must preserve the expansion deadline.
                    input.mouseMove(pane, targetPoint.x + step, targetPoint.y, 1, Qt.LeftButton)
                    break
                case 2: case 3: case 4: case 5:
                    input.mouseMove(pane, targetPoint.x + step, targetPoint.y, 1, Qt.LeftButton)
                    break
                case 6:
                    check(pane.collapsedDirectories.src === false, "Pointer hover did not expand folder")
                    input.mouseRelease(pane, targetPoint.x + step, targetPoint.y)
                    break
                case 7:
                    check(pane.draggingPath === "", "Release did not end drag")
                    check(controller.document_ids.indexOf("src/README.md") >= 0, "Native drop did not move file: " + controller.error_message)
                    check(pane.collapsedDirectories.src === false, "Successful drop closed destination")
                    var rootTarget = findItem(pane, "workspaceDropTarget")
                    check(rootTarget && rootTarget.visible && rootTarget.height >= 32,
                        "Workspace root drop block is missing")
                    var movedTree = findItem(pane, "fileTree")
                    check(rootTarget.parent !== movedTree, "Root drop block scrolls with tree rows")
                    movedTree.forceLayout()
                    var movedSource = movedTree.itemAtCell(Qt.point(0, pane.treeRowForPath("src/README.md")))
                    check(movedSource, "Moved source row is missing")
                    sourcePoint = movedSource.mapToItem(pane, 100, movedSource.height / 2)
                    targetPoint = rootTarget.mapToItem(pane, rootTarget.width / 2, rootTarget.height / 2)
                    input.mousePress(pane, sourcePoint.x, sourcePoint.y)
                    input.mouseMove(pane, sourcePoint.x + 20, sourcePoint.y, 1, Qt.LeftButton)
                    input.mouseMove(pane, targetPoint.x, targetPoint.y, 1, Qt.LeftButton)
                    break
                case 8:
                    check(pane.hoveringDrop && pane.hoverDirectory === "" && pane.validDrop,
                        "Workspace root block did not accept native drag")
                    input.mouseRelease(pane, targetPoint.x, targetPoint.y)
                    break
                case 9:
                    check(controller.document_ids.indexOf("README.md") >= 0
                        && controller.document_ids.indexOf("src/README.md") < 0,
                        "Dropping on workspace root did not move file back: " + controller.error_message)
                    console.log("KDE native pointer drag smoke passed")
                    Qt.exit(0)
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
