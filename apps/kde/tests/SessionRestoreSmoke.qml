import QtQuick
import QtQuick.Controls
import QtTest
import "../qml"
import org.nativeworkspace.Desktop

Main {
    id: window
    TestCase { id: input; when: false }
    property string mode: ""
    property string fixture: ""
    property int stage: 0
    property var driverController: null
    property var pane: null
    function check(value, message) { if (!value) throw new Error(message) }
    function done() {
        check(driverController.prepare_to_quit(false), driverController.error_message)
        console.log("KDE session restore (" + mode + ") smoke test passed")
        ApplicationLifecycle.finishQuit()
    }
    Timer {
        interval: 16
        running: true
        repeat: false
        property int ticks: 0
        onTriggered: {
            try {
                if (++ticks > 600) throw new Error("Session restore timed out")
                if (!driverController) {
                    driverController = input.findChild(window, "workspaceController")
                    pane = input.findChild(window, "workspacePane")
                    check(driverController && pane, "Main did not create its controller and pane")
                    for (const argument of Qt.application.arguments) {
                        if (argument.startsWith("--smoke-restore=")) mode = argument.slice(16)
                        if (argument.startsWith("--restore-fixture=")) fixture = argument.slice(18)
                    }
                    check(fixture.length > 0, "Pass --restore-fixture=/absolute/fixture")
                }
                driverController.poll()
                if (driverController.move_pending) { restart(); return }
                if (stage === 0 && mode === "seed") {
                    driverController.open_workspace(fixture)
                    driverController.open_document("README.md")
                    driverController.preview_document("src/main.rs")
                    driverController.open_files(["file://" + fixture + ".external.txt"])
                    stage = 1
                    restart()
                    return
                }
                if (mode === "seed") {
                    driverController.preview_document("README.md")
                    check(driverController.open_document_ids.length === 3, "Seed did not open three tabs")
                    done()
                    return
                }
                if (mode === "priority") {
                    check(driverController.open_document_ids.length === 1, "Cold OS request also restored old tabs")
                    check(driverController.active_document_id === fixture.split("/").pop() + ".external.txt", "Requested file did not take precedence")
                    done()
                    return
                }
                check(window.visible && !pane.showingHome, "Restored workspace was not presented")
                check(driverController.active_document_id === "README.md", "Active tab was not restored")
                if (mode === "missing") {
                    check(driverController.open_document_ids.length === 2 && driverController.error_message.length > 0, "Missing tab did not produce partial restoration")
                    done()
                    return
                }
                check(driverController.open_document_ids.length === 3, "Tab count was not restored")
                check(driverController.open_document_ids[0] === "README.md" && driverController.open_document_ids[1] === "src/main.rs", "Tab order was not restored")
                if (stage === 0) {
                    check(driverController.preview_document_id === "src/main.rs", "Preview was not restored")
                    check(driverController.error_message.length === 0, driverController.error_message)
                    pane.editorText = driverController.active_document_text + (mode === "discard" ? "\ndiscarded before quit" : "\nsaved before quit")
                    pane.requestQuit()
                    input.wait(100)
                    const dialog = input.findChild(pane, "quitDialog")
                    check(dialog && dialog.visible, "Quit confirmation was not presented")
                    input.findChild(pane, "cancelQuitAction").trigger()
                    input.tryCompare(dialog, "visible", false, 2000)
                    check(driverController.active_document_dirty && !dialog.visible, "Cancelling quit changed the buffer or left the dialog visible")
                    pane.requestQuit()
                    input.wait(100)
                    console.log("KDE restored order/preview/external tab/native Cancel and " + (mode === "discard" ? "Discard" : "Save All") + " smoke test passed")
                    input.findChild(pane, mode === "discard" ? "discardQuitAction" : "saveQuitAction").trigger()
                    stage = 2
                }
            } catch (error) {
                console.error(error)
                ApplicationLifecycle.finishQuit(1)
            }
        }
    }
}
