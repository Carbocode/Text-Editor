import QtQuick
import org.nativeworkspace.Desktop
import "SmokeLifecycle.js" as SmokeLifecycle
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
        if (item.objectName === name && (name !== "pendingMoveIndicator" || item.visible)) return item
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
    property int heartbeatCount: 0
    property double lastHeartbeat: 0
    property double maxHeartbeatGap: 0
    property double releaseTime: 0
    property bool delayed: Qt.application.arguments.some(value => value === "--smoke-drag-delay-ms=1000")
    Timer {
        interval: 16
        repeat: true
        running: releaseTime > 0 && step <= 8
        onTriggered: {
            var now = Date.now()
            if (lastHeartbeat > 0) maxHeartbeatGap = Math.max(maxHeartbeatGap, now - lastHeartbeat)
            lastHeartbeat = now
            ++heartbeatCount
        }
    }
    property int step: -2
    property bool releaseChecked: false
    property bool quickDropChecked: false
    property bool sawPendingIndicator: false
    property string localText: ""
    property string originalText: ""
    property var sourcePoint
    property var targetPoint
    Timer {
        interval: 100
        repeat: true
        running: true
        onTriggered: {
            try {
                switch (step++) {
                case -2:
                    controller.open_document("README.md")
                    originalText = controller.active_document_text
                    break
                case -1:
                    break
                case 0:
                    check(!findItem(pane, "workspaceDropTarget").visible, "Idle workspace shows drop box")
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
                    // Exercise a widget change not yet synchronized to the controller.
                    var editor = findItem(pane, "documentEditor")
                    localText = originalText + "\nedit immediately before drop"
                    pane.syncingEditor = true
                    editor.text = localText
                    pane.syncingEditor = false
                    check(controller.active_document_text === originalText, "Local edit unexpectedly flushed before drop")
                    if (delayed) controller.delay_next_background_job(1000)
                    var releasedAt = Date.now()
                    releaseTime = releasedAt
                    lastHeartbeat = releasedAt
                    input.mouseRelease(pane, targetPoint.x + step, targetPoint.y)
                    console.log("Drop handler duration=" + (Date.now() - releasedAt) + "ms documents=" + controller.document_ids.length)
                    check(Date.now() - releasedAt < 250, "Release blocked the GUI thread")
                    check(!findItem(pane, "sidebarDragGhost").visible, "Release retained the drag image until another frame")
                    check(controller.document_ids.indexOf("README.md") >= 0, "Release replaced tree rows during pointer delivery")
                    break
                case 7:
                    if (!releaseChecked) {
                        releaseChecked = true
                        check(controller.document_ids.indexOf("src/README.md") >= 0,
                            "Release did not relocate the row on the next turn: files=" + controller.document_ids
                                + ", error=" + controller.error_message + ", pending=" + controller.move_pending)
                        check(pane.treeRowForPath("src/README.md") >= 0, "Drop did not update the native tree on the next turn")
                        check(controller.active_document_text === localText, "Drop lost a local editor change")
                        check(controller.active_document_dirty, "Drop failed to retain the pending edit")
                        if (delayed) {
                            check(controller.move_pending, "Drop did not publish pending confirmation")
                            check(controller.pending_move_target === "", "A fresh move flashed a progress indicator")
                        }
                    }
                    if (controller.move_pending) {
                        if (controller.pending_move_target === "src/README.md") {
                            check(findItem(pane, "pendingMoveIndicator"), "Pending row has no progress indicator after painting")
                            sawPendingIndicator = true
                        }
                        --step
                        return
                    }
                    if (delayed) {
                        check(sawPendingIndicator, "A slow move never showed its progress indicator")
                        check(Date.now() - releaseTime >= 950, "Slow backend hook did not delay confirmation")
                        check(heartbeatCount >= 30 && maxHeartbeatGap < 250, "GUI timers froze during pending move: ticks=" + heartbeatCount + " gap=" + maxHeartbeatGap)
                        console.log("Slow backend GUI heartbeat ticks=" + heartbeatCount + " maximum gap=" + maxHeartbeatGap + "ms")
                    }
                    check(pane.draggingPath === "", "Release did not end drag")
                    check(controller.document_ids.indexOf("src/README.md") >= 0, "Native drop did not move file: " + controller.error_message)
                    check(pane.collapsedDirectories.src === false, "Successful drop closed destination")
                    var rootTarget = findItem(pane, "workspaceDropTarget")
                    check(rootTarget && !rootTarget.visible && rootTarget.height === 0,
                        "Workspace root drop block remained after drop")
                    check(!pane.hoveringDrop && !pane.validDrop, "Folder drop retained feedback")
                    pane.hoverDrop("src")
                    check(!pane.hoveringDrop, "Late hover restored folder feedback")
                    var movedTree = findItem(pane, "fileTree")
                    check(rootTarget.parent !== movedTree, "Root drop block scrolls with tree rows")
                    movedTree.forceLayout()
                    var movedSource = movedTree.itemAtCell(Qt.point(0, pane.treeRowForPath("src/README.md")))
                    check(movedSource, "Moved source row is missing")
                    sourcePoint = movedSource.mapToItem(pane, 100, movedSource.height / 2)
                    input.mousePress(pane, sourcePoint.x, sourcePoint.y)
                    input.mouseMove(pane, sourcePoint.x + 20, sourcePoint.y, 1, Qt.LeftButton)
                    check(rootTarget.visible && rootTarget.height > 0, "Drag did not show root drop block: visible="
                        + rootTarget.visible + ", height=" + rootTarget.height + ", path=" + pane.draggingPath)
                    targetPoint = rootTarget.mapToItem(pane, rootTarget.width / 2, rootTarget.height / 2)
                    input.mouseMove(pane, targetPoint.x, targetPoint.y, 1, Qt.LeftButton)
                    break
                case 8:
                    check(pane.hoveringDrop && pane.hoverDirectory === "" && pane.validDrop,
                        "Workspace root block did not accept native drag")
                    input.mouseRelease(pane, targetPoint.x, targetPoint.y)
                    break
                case 9:
                    if (controller.move_pending) { --step; return }
                    check(pane.draggingPath === "" && !pane.hoveringDrop && !pane.validDrop,
                        "Workspace root drop retained feedback")
                    check(!findItem(pane, "workspaceDropTarget").visible, "Root drop box remained visible")
                    pane.hoverDrop("")
                    check(!pane.hoveringDrop, "Late hover restored workspace feedback")
                    check(controller.document_ids.indexOf("README.md") >= 0
                        && controller.document_ids.indexOf("src/README.md") < 0,
                        "Dropping on workspace root did not move file back: " + controller.error_message)
                    pane.startEntryDrag("README.md", Qt.point(30, 30))
                    pane.hoverDrop("")
                    check(!pane.validDrop, "Same-parent drop should be forbidden")
                    check(!pane.dropEntry(""), "Same-parent drop was accepted")
                    check(pane.draggingPath === "" && !pane.hoveringDrop && !pane.validDrop,
                        "Rejected drop retained red feedback")
                    pane.startEntryDrag("README.md", Qt.point(30, 30))
                    pane.hoverDrop("src")
                    pane.finishEntryDrag(true)
                    check(!pane.hoveringDrop && !findItem(pane, "workspaceDropTarget").visible,
                        "Cancelled drag retained drop feedback")
                    pane.toggleDirectory("src")
                    check(pane.collapsedDirectories.src !== false, "Quick-drop destination must start closed")
                    var quickTree = findItem(pane, "fileTree")
                    quickTree.forceLayout()
                    var quickSource = quickTree.itemAtCell(Qt.point(0, pane.treeRowForPath("README.md")))
                    var quickTarget = quickTree.itemAtCell(Qt.point(0, pane.treeRowForPath("src")))
                    sourcePoint = quickSource.mapToItem(pane, 100, quickSource.height / 2)
                    targetPoint = quickTarget.mapToItem(pane, 100, quickTarget.height / 2)
                    input.mousePress(pane, sourcePoint.x, sourcePoint.y)
                    input.mouseMove(pane, sourcePoint.x + 20, sourcePoint.y, 1, Qt.LeftButton)
                    input.mouseMove(pane, targetPoint.x, targetPoint.y, 1, Qt.LeftButton)
                    input.mouseRelease(pane, targetPoint.x, targetPoint.y)
                    break
                case 10:
                    if (!quickDropChecked) {
                        quickDropChecked = true
                        check(pane.collapsedDirectories.src === false && pane.treeRowForPath("src/README.md") >= 0,
                            "Quick drop did not reveal the pending row on the next turn")
                    }
                    if (controller.move_pending) { --step; return }
                    pane.startEntryDrag("src/README.md", Qt.point(30, 30))
                    check(pane.dropEntry(""), "Quick-drop fixture could not be restored")
                    break
                case 11:
                    if (controller.move_pending) { --step; return }
                    check(controller.document_ids.indexOf("README.md") >= 0, "Quick-drop fixture restoration failed")
                    check(controller.active_document_text === originalText + "\nedit immediately before drop", "Confirmation lost the local edit")
                    controller.change_document(originalText)
                    controller.save_document()
                    console.log("KDE native pointer drag smoke passed")
                    SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
                }
            } catch (error) { console.error(error); SmokeLifecycle.quit(controller, ApplicationLifecycle, 1) }
        }
    }
}
