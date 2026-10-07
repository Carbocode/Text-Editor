import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.nativeworkspace.Diagnostic

ApplicationWindow {
    visible: true
    width: 360
    height: 200
    title: "Native Workspace"

    AppController { id: controller }

    Component.onCompleted: {
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            controller.increment()
            if (controller.counter !== 1 || controller.revision !== 1)
                throw new Error("CXX-Qt diagnostic state did not update")
            console.log("QML → CXX-Qt → Rust smoke test passed")
        }
    }

    ColumnLayout {
        anchors.centerIn: parent
        spacing: 16
        Label { text: "Counter: " + controller.counter }
        Button {
            text: "Increment"
            onClicked: controller.increment()
        }
    }
}
