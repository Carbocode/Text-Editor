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
    property real initialEditorFontSize: 0
    property bool trackingTabSelection: false
    property var tabSelections: []
    property var retainedTab: null
    Connections {
        target: pane.documentTabBar
        function onCurrentItemChanged() {
            if (window.trackingTabSelection && pane.documentTabBar.currentItem)
                window.tabSelections.push(pane.documentTabBar.currentItem.path)
        }
    }
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
                    var editor = input.findChild(pane, "documentEditor")
                    editor.insert(editor.length, "x")
                    editor.remove(editor.length - 1, editor.length)
                    editor.select(0, 2)
                    initialEditorFontSize = pane.editorFont.pointSize
                    input.findChild(pane, "increaseTextSizeAction").trigger()
                    initialWidth = pane.sidebar.width
                    resizeSidebarTo(initialWidth + Kirigami.Units.gridUnit * 4)
                    break
                case 2:
                    var editor = input.findChild(pane, "documentEditor")
                    check(pane.editorFont.pointSize > initialEditorFontSize, "Increase Text Size did not resize the editor")
                    check(editor.selectedText === localText.substring(0, 2) && editor.canUndo,
                        "Changing text size altered selection or cleared undo")
                    check(editor.text === localText, "Changing text size altered the document")
                    input.findChild(pane, "decreaseTextSizeAction").trigger()
                    check(pane.editorFont.pointSize === initialEditorFontSize, "Decrease Text Size did not restore the editor font")
                    console.log("KDE text size/content/selection/undo smoke test passed")
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
                    controller.save_all_documents()
                    controller.create_entry("stable-a.txt", false)
                    controller.create_entry("stable-b.txt", false)
                    controller.open_document("stable-a.txt")
                    controller.open_document("stable-b.txt")
                    break
                case 6:
                    controller.open_document("stable-a.txt")
                    retainedTab = pane.documentTabBar.currentItem
                    trackingTabSelection = true
                    controller.create_entry("stable-c.txt", false)
                    controller.open_document("stable-c.txt")
                    controller.close_document("stable-c.txt")
                    controller.close_document("src/main.rs")
                    break
                case 7:
                    trackingTabSelection = false
                    check(tabSelections.length > 0 && tabSelections.every(path => path === "stable-c.txt" || path === "stable-b.txt"),
                        "Native tab selection visited an unrelated first tab: " + tabSelections)
                    check(pane.documentTabBar.itemAt(controller.open_document_ids.indexOf("stable-a.txt")) === retainedTab,
                        "Opening or closing tabs recreated an existing native tab")
                    input.mouseClick(retainedTab, retainedTab.width / 2, retainedTab.height / 2)
                    check(controller.active_document_id === "stable-a.txt", "Clicking the retained native tab did not select it")
                    controller.change_document("unsaved marker check")
                    console.log("KDE stable document tab selection/retained delegates smoke test passed")
                    break
                case 8:
                    check(pane.documentTabBar.currentItem === retainedTab && retainedTab.checked,
                        "Native tab highlight did not follow the active document")
                    controller.create_entry("preview-a.txt", false)
                    controller.create_entry("preview-b.txt", false)
                    controller.preview_document("preview-a.txt")
                    break
                case 9:
                    check(controller.preview_document_id === "preview-a.txt" && pane.documentTabBar.currentItem.font.italic,
                        "Preview tab is missing its italic title")
                    controller.preview_document("preview-b.txt")
                    check(controller.open_document_ids.indexOf("preview-a.txt") === -1,
                        "Opening another preview kept the old temporary tab")
                    var previewTab = pane.documentTabBar.currentItem
                    input.mouseDoubleClickSequence(previewTab, previewTab.width / 2, previewTab.height / 2)
                    break
                case 10:
                    check(controller.preview_document_id.length === 0 && !pane.documentTabBar.currentItem.font.italic,
                        "Double-clicking a native tab did not keep it open")
                    controller.preview_document("preview-a.txt")
                    controller.change_document("edited preview")
                    check(controller.preview_document_id.length === 0, "Editing did not promote the preview")
                    controller.save_all_documents()
                    controller.preview_document("stable-c.txt")
                    check(controller.open_document_ids.indexOf("preview-a.txt") !== -1
                        && controller.open_document_ids.indexOf("preview-b.txt") !== -1,
                        "A promoted preview was replaced")
                    console.log("KDE preview replacement/italic/double-click/edit promotion smoke test passed")
                    break
                case 11:
                    check(pane.documentTabBar.currentItem.path === controller.active_document_id
                        && pane.documentTabBar.currentItem.checked && pane.documentTabBar.currentItem.font.italic,
                        "Native tab selection did not follow the new preview: " + pane.documentTabBar.currentItem.path + " vs " + controller.active_document_id)
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
