import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic

Kirigami.ApplicationWindow {
    visible: true
    width: 360
    height: 200
    title: "Native Workspace"

    AppController { id: controller }

    Component.onCompleted: {
        if (Qt.application.arguments.indexOf("--smoke") !== -1) {
            controller.increment()
            if (controller.counter !== 1 || controller.revision !== 1)
                throw new Error("Kirigami diagnostic state did not update")
            console.log("Kirigami → CXX-Qt → Rust smoke test passed")
        }
    }

    pageStack.initialPage: Kirigami.Page {
        ColumnLayout {
            anchors.centerIn: parent
            spacing: Kirigami.Units.largeSpacing
            Label { text: "Counter: " + controller.counter }
            Button {
                text: "Increment"
                onClicked: controller.increment()
            }
        }
    }
}
