import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic

Kirigami.ApplicationWindow {
    visible: !workspacePane.showingHome
    width: Kirigami.Units.gridUnit * 60
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 36
    minimumHeight: Kirigami.Units.gridUnit * 24
    title: controller.workspace_name.length > 0 ? controller.workspace_name : "Native Workspace"

    function finishSmoke(window) {
        var capture = Qt.application.arguments.find(argument => argument.startsWith("--smoke-screenshot="))
        if (!capture) {
            Qt.quit()
            return
        }
        var capturing = window.pageStack.grabToImage(function(result) {
            if (result.saveToFile(capture.substring("--smoke-screenshot=".length))) Qt.quit()
            else Qt.exit(1)
        })
        if (!capturing) Qt.exit(1)
    }

    globalDrawer: Kirigami.GlobalDrawer {
        title: "Native Workspace"
        titleIcon: "accessories-text-editor"
        isMenu: true
        actions: workspacePane.applicationActions
    }
    AppController { id: controller }
    Timer { id: smokeLayoutDelay; interval: 250; repeat: false }
    Timer { id: benchmarkDelay; interval: 200; repeat: false }
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
            anchors.fill: parent
            controller: controller
            enableBrowse: true
            onBrowseRequested: folderDialog.open()
        }
    }

    Component.onCompleted: {
        if (Qt.application.arguments.indexOf("--smoke") === -1 && !Qt.application.arguments.some(argument => argument.startsWith("--smoke-workspace=") || argument.startsWith("--benchmark-workspace=")))
            controller.initialize_persistence()
        for (var benchmarkArgument of Qt.application.arguments) {
            if (benchmarkArgument.startsWith("--benchmark-workspace=")) {
                var openedAt = Date.now()
                controller.open_workspace(benchmarkArgument.substring("--benchmark-workspace=".length))
                var openMs = Date.now() - openedAt
                var initialAt = Date.now()
                var initialRows = workspacePane.sidebarEntries()
                var initialMs = Date.now() - initialAt
                benchmarkDelay.triggered.connect(function() {
                    if (!workspacePane.treeMatchesSidebarEntries())
                        throw new Error("Native tree did not show the initial rows")
                    var expandedAt = Date.now()
                    workspacePane.toggleDirectory("src")
                    var expandedRows = workspacePane.sidebarEntries()
                    var expandedChildPath = expandedRows.find(entry => entry.path.startsWith("src/")).path
                    var expandMs = Date.now() - expandedAt
                    if (expandedRows.length <= initialRows.length)
                        throw new Error("Benchmark directory did not expand; expected a populated src folder")
                    Qt.callLater(function() {
                        if (!workspacePane.treeMatchesSidebarEntries())
                            throw new Error("Native tree did not expand with the folder")
                        var expandedSettledMs = Date.now() - expandedAt
                        var collapsedAt = Date.now()
                        workspacePane.toggleDirectory("src")
                        var collapsedRows = workspacePane.sidebarEntries()
                        var collapseMs = Date.now() - collapsedAt
                        if (collapsedRows.length !== initialRows.length)
                            throw new Error("Benchmark directory did not collapse")
                        Qt.callLater(function() {
                            if (!workspacePane.treeMatchesSidebarEntries() || workspacePane.treeRowForPath(expandedChildPath) !== -1)
                                throw new Error("Native tree did not collapse with the folder")
                            console.log("KDE tree benchmark files=" + controller.document_ids.length
                                + " open=" + openMs + "ms initial=" + initialMs + "ms/" + initialRows.length
                                + " expand=" + expandMs + "ms/" + expandedRows.length
                                + " expand-next-turn=" + expandedSettledMs + "ms"
                                + " collapse=" + collapseMs + "ms/" + collapsedRows.length
                                + " collapse-next-turn=" + (Date.now() - collapsedAt) + "ms")
                            workspacePane.filterNamesNow(expandedChildPath.split("/").pop())
                            workspacePane.sidebarEntries()
                            Qt.callLater(function() {
                                if (!workspacePane.treeMatchesSidebarEntries())
                                    throw new Error("Native tree did not show filtered files")
                                Qt.quit()
                            })
                        })
                    })
                })
                benchmarkDelay.start()
                return
            }
        }
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            controller.increment()
            if (controller.counter !== 1 || controller.revision !== 1)
                throw new Error("CXX-Qt diagnostic state did not update")
            smokeLayoutDelay.triggered.connect(function() {
                if (!workspacePane.homeWindow.visible || !workspacePane.homeWindow.pageStack.currentItem)
                    throw new Error("Kirigami Home page was not created")
                console.log("Kirigami → CXX-Qt → Rust smoke test passed")
                finishSmoke(workspacePane.homeWindow)
            })
            smokeLayoutDelay.start()
        }
        for (var i = 0; i < Qt.application.arguments.length; ++i) {
            var argument = Qt.application.arguments[i]
            if (argument.startsWith("--smoke-workspace=")) {
                if (!workspacePane.showingHome)
                    throw new Error("Home was not shown at startup")
                controller.open_workspace(argument.substring("--smoke-workspace=".length))
                if (workspacePane.showingHome)
                    throw new Error("Home did not yield to the workspace window")
                if (controller.document_ids.length === 0)
                    throw new Error("Workspace documents were not listed")
                if (workspacePane.homeWindow === workspacePane.Window.window)
                    throw new Error("Home and workspace must use different native windows")
                workspacePane.showingHome = true
                if (!workspacePane.homeWindow.visible || visible)
                    throw new Error("Returning Home did not switch native windows")
                workspacePane.showingHome = false
                if (workspacePane.homeWindow.visible || !visible)
                    throw new Error("Resuming did not restore the workspace window")
                if (title !== controller.workspace_name)
                    throw new Error("Window title did not follow the workspace")
                if (controller.directory_ids.indexOf("src") !== -1) {
                    if (workspacePane.sidebarEntries()[0].path !== "src")
                        throw new Error("Directories were not ordered before files")
                    if (workspacePane.sidebarEntries().some(entry => entry.path === "src/main.rs"))
                        throw new Error("Directory contents were visible before expansion")
                    workspacePane.toggleDirectory("src")
                    if (!workspacePane.sidebarEntries().some(entry => entry.path === "src/main.rs"))
                        throw new Error("Directory contents did not appear after expansion")
                    if (controller.directory_ids.indexOf("src/nested") !== -1) {
                        var expanded = workspacePane.sidebarEntries()
                        var nestedIndex = expanded.findIndex(entry => entry.path === "src/nested")
                        var mainIndex = expanded.findIndex(entry => entry.path === "src/main.rs")
                        if (nestedIndex < 0 || nestedIndex >= mainIndex)
                            throw new Error("Nested directories were not ordered before files")
                        if (expanded.some(entry => entry.path === "src/nested/inside.txt"))
                            throw new Error("Nested directory contents were visible before expansion")
                        workspacePane.toggleDirectory("src/nested")
                        if (!workspacePane.sidebarEntries().some(entry => entry.path === "src/nested/inside.txt"))
                            throw new Error("Nested directory contents did not appear after expansion")
                    }
                }
                workspacePane.filterNamesNow("main")
                if (!workspacePane.sidebarEntries().some(entry => entry.path === "src/main.rs")
                    || workspacePane.sidebarEntries().some(entry => entry.path === "README.md"))
                    throw new Error("Name filter did not isolate matching files")
                workspacePane.filterNamesNow("src")
                if (!workspacePane.sidebarEntries().some(entry => entry.path === "src"))
                    throw new Error("Name filter did not find a folder")
                workspacePane.filterNamesNow("")
                workspacePane.sidebarTab = 1
                workspacePane.scheduleSidebarUpdate()
                controller.search_files("unlikely-no-match")
                if (controller.search_result_ids.length !== 0)
                    throw new Error("Filename search did not filter results")
                if (workspacePane.sidebarEntries().length !== 0)
                    throw new Error("Sidebar kept stale entries after search")
                controller.search_files("README")
                if (workspacePane.sidebarEntries().length !== 0)
                    throw new Error("Text search included a filename-only match")
                controller.search_files("")
                controller.open_document(controller.document_ids[0])
                workspacePane.editorText = "QML Phase 1 smoke content\n" + "filler\n".repeat(80) + "💡 Another Phase 1 match"
                if (!controller.active_document_dirty)
                    throw new Error("Document did not become dirty")
                var unsavedContent = workspacePane.editorText
                workspacePane.showingHome = true
                workspacePane.showingHome = false
                if (workspacePane.editorText !== unsavedContent || !controller.active_document_dirty)
                    throw new Error("Home navigation discarded unsaved editor state")
                workspacePane.saveActive()
                if (controller.active_document_dirty)
                    throw new Error("Document remained dirty after save")
                controller.search_files_now("phase 1")
                if (controller.search_result_ids.length !== 2 || controller.search_result_snippets[0] !== "QML Phase 1 smoke content")
                    throw new Error("Content search did not return a matching preview")
                if (workspacePane.searchFileCount !== 1)
                    throw new Error("Search matches were not grouped by file")
                if (workspacePane.sidebarEntries().length !== 1)
                    throw new Error("Sidebar did not update its cached search entries")
                if (workspacePane.matchIndicesForFile(controller.document_ids[0]).length !== 2)
                    throw new Error("Cached search matches did not include both occurrences")
                smokeLayoutDelay.triggered.connect(function() {
                    workspacePane.openSearchResult(1, controller.document_ids[0])
                    Qt.callLater(function() {
                        if (workspacePane.editorSelectedText !== "Phase 1" || workspacePane.editorScrollY <= 0)
                            throw new Error("Selecting a search occurrence did not select its text")
                        workspacePane.createEntry("notes", true)
                        workspacePane.createEntry("notes/todo.md", false)
                        if (controller.document_ids.indexOf("notes/todo.md") === -1)
                            throw new Error("New file did not appear in workspace")
                        console.log("Kirigami workspace edit/save smoke test passed")
                        finishSmoke(workspacePane.Window.window)
                    })
                })
                smokeLayoutDelay.start()
            }
        }
    }
}
