import QtQuick
import QtQuick.Controls
import QtTest
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic
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
    property real initialWidth: 0
    property real initialDocumentTabWidth: 0
    property string localText: "// unsaved resize check\n"

    function check(condition, message) { if (!condition) throw new Error(message) }
    function checkLayout() {
        var search = pane.activeSidebarSearchRow
        var documents = pane.documentTabBar
        check(Math.abs(search.height - documents.height) <= 1,
            "Search and document tab rows have different heights: " + search.height + " vs " + documents.height)
        check(Math.abs(search.mapToItem(pane, 0, 0).y - documents.mapToItem(pane, 0, 0).y) <= 1,
            "Search and document tabs are not vertically aligned")
        for (var bar of [pane.sidebarTabBar, documents]) {
            for (var i = 0; i < bar.count; i++) {
                var button = bar.itemAt(i)
                check(button.y >= -1 && button.y + button.height <= bar.availableHeight + 1,
                    "Tab button is positioned outside the available height")
                check(Math.abs(button.height - bar.availableHeight) <= 1,
                    "Tab button did not fit the available height: " + button.height + " vs " + bar.availableHeight)
            }
        }
        var header = pane.sidebarHeaderBar
        check(header.contentItem === pane.sidebarTabBar, "Sidebar tabs are missing from the header")
        var editorHeader = pane.workspacePage.globalToolBarItem
        check(header && editorHeader && header.height > 0 && editorHeader.height > 0,
            "Missing native sidebar/editor header")
        var sidebarRight = header.mapToItem(pane, header.width, 0)
        var editorLeft = editorHeader.mapToItem(pane, 0, 0)
        check(Math.abs(sidebarRight.x - editorLeft.x) <= 1,
            "Header divider is not aligned with sidebar: " + sidebarRight.x + " vs " + editorLeft.x)
        check(Math.abs(sidebarRight.y - editorLeft.y) <= 1
            && Math.abs(header.height - editorHeader.height) <= 1,
            "Sidebar and editor headers are not vertically aligned")
        check(Math.abs(header.width - pane.sidebar.width) <= 1,
            "Sidebar header did not follow drawer width")
    }
    function resizeSidebarTo(x, fromHeader) {
        var content = pane.sidebar.contentItem
        var edge = content.mapToItem(pane, content.width,
            fromHeader ? pane.sidebarHeaderBar.height / 2 : content.height / 2)
        input.mouseMove(pane, edge.x, edge.y)
        input.mousePress(pane, edge.x, edge.y)
        check(pane.sidebar.interactiveResizing, "Native Kirigami resize handle did not receive the press")
        input.mouseMove(pane, x, edge.y, 10, Qt.LeftButton)
        input.mouseRelease(pane, x, edge.y)
    }
    Component.onCompleted: {
        var argument = Qt.application.arguments.find(value => value.startsWith("--smoke-sidebar-layout="))
        controller.open_workspace(argument.substring("--smoke-sidebar-layout=".length))
        pane.showingHome = false
    }
    Timer {
        interval: 250
        repeat: true
        running: true
        onTriggered: {
            try {
                switch (step++) {
                case 0:
                    pane.toggleDirectory("src")
                    controller.open_document("src/main.rs")
                    break
                case 1:
                    checkLayout()
                    check(pane.sidebar.interactiveResizeEnabled && !pane.sidebar.modal,
                        "Sidebar is not a resizable native desktop drawer")
                    initialDocumentTabWidth = pane.documentTabBar.currentItem.width
                    controller.change_document(localText)
                    initialWidth = pane.sidebar.width
                    resizeSidebarTo(initialWidth + Kirigami.Units.gridUnit * 4)
                    break
                case 2:
                    check(Math.abs(pane.documentTabBar.currentItem.width - initialDocumentTabWidth) <= 0.1,
                        "Dirty indicator changed the document tab width")
                    var tab = pane.documentTabBar.currentItem
                    var marker = tab.children.find(item => item.objectName === "dirtyDocumentTab:src/main.rs")
                    check(marker && marker.visible && marker.x + marker.width <= tab.leftPadding,
                        "Dirty indicator is missing from the fixed leading padding")
                    check(tab.text === "main.rs", "Dirty indicator leaked into the document title")
                    check(pane.sidebar.width > initialWidth + Kirigami.Units.gridUnit * 2,
                        "Dragging the native border did not widen the sidebar")
                    checkLayout()
                    resizeSidebarTo(Kirigami.Units.gridUnit * 4, true)
                    break
                case 3:
                    check(Math.abs(pane.sidebar.width - pane.sidebar.minimumSize) <= 1,
                        "Native drawer did not enforce its minimum width")
                    checkLayout()
                    pane.sidebarTab = 1
                    window.width = 800
                    break
                case 4:
                    checkLayout()
                    check(pane.sidebarTabBar.currentIndex === 1, "Header tab selection did not follow the sidebar")
                    pane.sidebarTab = 0
                    resizeSidebarTo(initialWidth)
                    break
                case 5:
                    checkLayout()
                    check(pane.collapsedDirectories.src === false, "Resizing lost directory expansion")
                    check(controller.active_document_text === localText && controller.active_document_dirty,
                        "Resizing lost the unsaved editor buffer")
                    console.log("KDE native sidebar resize/header alignment smoke test passed")
                    running = false
                    var capture = Qt.application.arguments.find(value => value.startsWith("--smoke-screenshot="))
                    if (!capture) { Qt.quit(); return }
                    check(window.pageStack.grabToImage(function(result) {
                        if (result.saveToFile(capture.substring("--smoke-screenshot=".length))) Qt.quit()
                        else Qt.exit(1)
                    }), "Could not capture the native sidebar layout")
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
