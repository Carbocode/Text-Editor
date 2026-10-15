import QtQuick
import QtQuick.Controls
import QtTest
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic
import org.nativeworkspace.Test
import "../qml"

Kirigami.ApplicationWindow {
    id: window
    width: 1000
    height: 650
    visible: true
    globalDrawer: Kirigami.GlobalDrawer { isMenu: true; actions: pane.applicationActions }
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None
    pageStack.initialPage: Kirigami.Page {
        padding: 0
        WorkspacePane { id: pane; anchors.fill: parent; controller: controller }
    }
    AppController { id: controller }
    TestCase { id: input; when: false }
    property int step: 0
    property var currentItem
    property var currentPoint
    function findItem(item, name) {
        if (item.objectName === name) return item
        for (var child of item.children || []) {
            var found = findItem(child, name)
            if (found) return found
        }
        return null
    }
    function findButton(item, text) {
        if (item.visible && item.enabled && item.text === text && item.clicked !== undefined) return item
        for (var child of item.children || []) {
            var found = findButton(child, text)
            if (found) return found
        }
        return null
    }
    function moveOver(item, x, y) {
        if (!item) throw new Error("Missing cursor target")
        currentItem = item
        var point = item.mapToItem(pane, x === undefined ? item.width / 2 : x,
            y === undefined ? item.height / 2 : y)
        currentPoint = point
        input.mouseMove(pane, point.x, point.y)
    }
    function expectCursor(name, expected) {
        var actual = CursorProbe.shape(window)
        if (actual !== expected) throw new Error(name + " cursor=" + actual + " expected=" + expected
            + " " + CursorProbe.describe(window, currentItem))
        console.log("KDE cursor " + name + "=" + actual)
    }
    Component.onCompleted: {
        var arg = Qt.application.arguments.find(value => value.startsWith("--smoke-cursors="))
        controller.open_workspace(arg.substring("--smoke-cursors=".length))
        pane.showingHome = false
    }
    Timer {
        interval: 250
        running: true
        repeat: true
        onTriggered: {
            try {
                switch (step++) {
                case 0: controller.open_document("src/main.rs"); break
                case 1: moveOver(findItem(pane, "documentEditor"), 60, 15); break
                case 2:
                    expectCursor("editor", Qt.IBeamCursor)
                    var content = pane.sidebar.contentItem
                    moveOver(content, content.width, content.height / 2)
                    break
                case 3:
                    expectCursor("resize", Qt.SplitHCursor)
                    var tree = findItem(pane, "fileTree")
                    moveOver(tree.itemAtCell(Qt.point(0, pane.treeRowForPath("README.md"))))
                    break
                case 4:
                    expectCursor("file", Qt.PointingHandCursor)
                    moveOver(findItem(pane, "sidebarTabs"))
                    break
                case 5:
                    expectCursor("tabs", Qt.PointingHandCursor)
                    moveOver(findItem(pane, "fileNameFilter"), 60)
                    break
                case 6:
                    expectCursor("search", Qt.IBeamCursor)
                    controller.change_document("// cursor smoke unsaved buffer\n")
                    break
                case 7:
                    moveOver(findButton(pane.workspacePage.globalToolBarItem, "Save"))
                    break
                case 8:
                    expectCursor("toolbar button", Qt.PointingHandCursor)
                    moveOver(findItem(pane, "editorLineNumbers"), 10, 20)
                    break
                case 9:
                    expectCursor("gutter", Qt.IBeamCursor)
                    moveOver(findItem(pane, "editorMinimap"))
                    break
                case 10:
                    expectCursor("minimap", Qt.OpenHandCursor)
                    input.mousePress(pane, currentPoint.x, currentPoint.y)
                    break
                case 11:
                    expectCursor("minimap drag", Qt.ClosedHandCursor)
                    input.mouseRelease(pane, currentPoint.x, currentPoint.y)
                    break
                case 12:
                    expectCursor("minimap release", Qt.OpenHandCursor)
                    moveOver(pane.sidebarHeaderBar)
                    break
                case 13:
                    expectCursor("plain header", Qt.ArrowCursor)
                    console.log("KDE native cursor feedback smoke test passed")
                    Qt.quit()
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
