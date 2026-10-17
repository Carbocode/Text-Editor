import QtQuick
import org.nativeworkspace.Desktop
import "SmokeLifecycle.js" as SmokeLifecycle
import QtTest
import "../qml"
import QtQuick.Controls
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic

Kirigami.ApplicationWindow {
    visible: !workspacePane.showingHome
    width: Kirigami.Units.gridUnit * 72
    height: Kirigami.Units.gridUnit * 46
    minimumWidth: Kirigami.Units.gridUnit * 36
    minimumHeight: Kirigami.Units.gridUnit * 24
    title: controller.workspace_name.length > 0 ? controller.workspace_name : "Native Workspace"

    function finishSmoke(window) {
        var capture = Qt.application.arguments.find(argument => argument.startsWith("--smoke-screenshot="))
        if (!capture) {
            SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
            return
        }
        var capturing = window.pageStack.grabToImage(function(result) {
            if (result.saveToFile(capture.substring("--smoke-screenshot=".length))) SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
            else SmokeLifecycle.quit(controller, ApplicationLifecycle, 1)
        })
        if (!capturing) SmokeLifecycle.quit(controller, ApplicationLifecycle, 1)
    }

    globalDrawer: Kirigami.GlobalDrawer {
        title: "Native Workspace"
        titleIcon: "accessories-text-editor"
        isMenu: true
        actions: workspacePane.applicationActions
    }
    AppController { id: controller }
    TestCase { id: input; when: false }
    function filterNames(query) {
        workspacePane.fileNameQuery = query
        input.wait(250)
        controller.poll()
    }
    function search(query) {
        controller.search_files(query)
        settleSearch()
    }
    function settleSearch() {
        var deadline = Date.now() + 5000
        do {
            input.wait(5)
            controller.poll()
            if (Date.now() > deadline) throw new Error("Search/replacement did not settle")
        } while (controller.move_pending || controller.search_in_progress)
    }
    function searchAndReplace() {
        workspacePane.createEntry("search-panel.txt", false)
        workspacePane.editorText = "cat CAT Cat concatenate 💡 cat"
        workspacePane.saveActive()
        controller.configure_search_files("search-panel.txt", "", true, false)
        controller.configure_search(false, true, false)
        search("cat")
        if (controller.search_results.length !== 4) throw new Error("Whole-word search failed")
        controller.configure_search(true, true, false)
        settleSearch()
        if (controller.search_results.length !== 2) throw new Error("Case-sensitive search failed")
        controller.configure_search(false, true, true)
        search("[")
        if (controller.search_error.length === 0 || controller.search_results.length !== 0)
            throw new Error("Invalid regex did not clear results")
        controller.configure_search(false, true, false)
        search("cat")
        controller.prepare_replacement("dog", true, "", -1)
        settleSearch()
        if (controller.replacement_preview.length === 0 || controller.replacement_occurrences !== 4)
            throw new Error("Replacement preview was not published")
        if (workspacePane.editorText !== "cat CAT Cat concatenate 💡 cat")
            throw new Error("Preview changed the document")
        controller.apply_replacement()
        settleSearch()
        if (controller.replacement_preview.length !== 0 || !controller.active_document_dirty
                || workspacePane.editorText !== "dog DOG Dog concatenate 💡 dog")
            throw new Error("Replacement did not preserve case and unsaved state")
        workspacePane.saveActive()
        search("dog")
        controller.prepare_replacement("fox", false, "", -1)
        settleSearch()
        workspacePane.editorText = "dog typed after preview"
        workspacePane.flushEditor()
        controller.apply_replacement()
        settleSearch()
        if (controller.error_message.length === 0 || workspacePane.editorText !== "dog typed after preview")
            throw new Error("A stale preview overwrote the buffer")
        controller.discard_replacement()
        console.log("Kirigami Search options/Unicode/replace preview/stale buffer smoke test passed")
    }
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
                            filterNames(expandedChildPath.split("/").pop())
                            workspacePane.sidebarEntries()
                            Qt.callLater(function() {
                                if (!workspacePane.treeMatchesSidebarEntries())
                                    throw new Error("Native tree did not show filtered files")
                                SmokeLifecycle.quit(controller, ApplicationLifecycle, 0)
                            })
                        })
                    })
                })
                benchmarkDelay.start()
                return
            }
        }
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            if (controller.revision !== 0)
                throw new Error("CXX-Qt initial state is invalid")
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
                filterNames("main")
                if (!workspacePane.sidebarEntries().some(entry => entry.path === "src/main.rs")
                    || workspacePane.sidebarEntries().some(entry => entry.path === "README.md"))
                    throw new Error("Name filter did not isolate matching files")
                filterNames("src")
                if (!workspacePane.sidebarEntries().some(entry => entry.path === "src"))
                    throw new Error("Name filter did not find a folder")
                filterNames("")
                workspacePane.sidebarTab = 1
                workspacePane.scheduleSidebarUpdate()
                controller.search_files("unlikely-no-match")
                if (controller.search_results.length !== 0)
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
                search("phase 1")
                if (controller.search_results.length !== 2 || controller.search_results[0].snippet !== "QML Phase 1 smoke content")
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
                        searchAndReplace()
                        finishSmoke(workspacePane.Window.window)
                    })
                })
                smokeLayoutDelay.start()
            }
        }
    }
}
