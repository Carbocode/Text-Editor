import QtQuick
import QtQuick.Controls
import org.nativeworkspace.Diagnostic
import org.nativeworkspace.Desktop
import org.nativeworkspace.Test

ApplicationWindow {
    visible: true
    width: 640
    height: 480
    AppController { id: controller }
    property string fixture: ""
    property int stage: 0
    function check(value, message) { if (!value) throw new Error(message) }
    Connections {
        target: SystemOpenRequests
        function onReceived() { controller.open_paths(SystemOpenRequests.takePending()) }
    }
    Component.onCompleted: {
        for (const argument of Qt.application.arguments) {
            if (argument.startsWith("--smoke-system-open=")) fixture = argument.slice(20)
        }
        const urls = SystemOpenRequests.takePending()
        check(urls.length === 1, "Pass the fixture README.md as a positional launch argument")
        controller.open_paths(urls)
    }
    Timer {
        interval: 16
        running: true
        repeat: true
        property int ticks: 0
        onTriggered: {
            try {
                if (++ticks > 600) throw new Error("System open timed out")
                controller.poll()
                if (controller.move_pending) return
                if (stage === 0) {
                    check(controller.error_message.length === 0, controller.error_message)
                    check(controller.active_document_id === "README.md", "Cold file did not open")
                    check(controller.preview_document_id.length === 0, "OS open created a preview")
                    controller.change_document("unsaved system-open smoke")
                    stage = 1
                    SystemOpenProbe.deliver("file://" + fixture + "/src")
                } else if (stage === 1) {
                    check(controller.error_message.length > 0, "Dirty folder switch was allowed")
                    check(controller.active_document_text === "unsaved system-open smoke", "Lost edits")
                    stage = 2
                    SystemOpenProbe.deliver("file://" + fixture + "/README.md")
                } else {
                    check(controller.open_document_ids.length === 1, "Repeated delivery duplicated the tab")
                    check(controller.active_document_text === "unsaved system-open smoke", "Reopen lost edits")
                    console.log("KDE system-open launch arguments/native event/dirty-buffer smoke test passed")
                    Qt.quit()
                }
            } catch (error) {
                console.error(error)
                Qt.exit(1)
            }
        }
    }
}
