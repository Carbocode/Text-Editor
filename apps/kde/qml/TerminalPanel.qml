import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Terminal 1.0

Item {
    id: root
    required property var controller
    readonly property var state: controller.terminal_json.length ? JSON.parse(controller.terminal_json) : ({tabs:[],active:null})
    property var closing: null
    property var tabsModel: []
    onStateChanged: if (JSON.stringify(state.tabs) !== JSON.stringify(tabsModel)) tabsModel = state.tabs
    function toggle() {
        if (state.tabs.length === 0) controller.terminal_action("new",0,false)
        visible = !visible
        surface.forceActiveFocus()
    }
    RowLayout {
        anchors.fill: parent
        spacing: 0
        ColumnLayout {
            Layout.fillHeight: true
            Layout.fillWidth: true
            spacing: 0
            TerminalSurface {
                id: surface
                objectName: "terminalSurface"
                Layout.fillHeight: true
                Layout.fillWidth: true
                snapshot: root.controller.terminal_json
                onKeyInput: (key,text,modifiers,paste) => { if (root.state.active !== null) root.controller.terminal_input(root.state.active,key,text,modifiers,paste) }
                onResized: (cols,rows) => Qt.callLater(function() { if (root.state.active !== null) root.controller.terminal_resize(root.state.active,cols,rows) })
                onScrolled: lines => { if (root.state.active !== null) root.controller.terminal_scroll(root.state.active,lines) }
            }
            Label {
                Layout.fillWidth: true
                text: { const tab=root.state.tabs.find(t => t.id===root.state.active); return tab ? tab.status : "" }
                visible: text.length>0
            }
        }
        Kirigami.Separator { Layout.fillHeight: true }
        ColumnLayout {
            Layout.fillHeight: true
            Layout.fillWidth: false
            Layout.preferredWidth: Kirigami.Units.gridUnit * 10
            spacing: 0
            RowLayout {
                Layout.alignment: Qt.AlignRight
                ToolButton { icon.name: "list-add"; display: AbstractButton.IconOnly; text: "New terminal"; ToolTip.text: text; ToolTip.visible: hovered; onClicked: root.controller.terminal_action("new",0,false) }
            }
            ListView {
                Layout.fillHeight: true
                Layout.fillWidth: true
                clip: true
                model: root.tabsModel
                delegate: Kirigami.SubtitleDelegate {
                    id: tabDelegate
                    required property var modelData
                    width: ListView.view.width
                    text: modelData.title
                    icon.name: "utilities-terminal"
                    highlighted: root.state.active === modelData.id
                    onClicked: root.controller.terminal_action("activate",modelData.id,false)
                    rightPadding: closeButton.width
                    ToolButton {
                        id: closeButton
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        icon.name: "window-close"
                        display: AbstractButton.IconOnly
                        text: "Close terminal"
                        Accessible.name: text
                        onClicked: {
                            if (tabDelegate.modelData.busy) { root.closing=tabDelegate.modelData.id; closeDialog.open() }
                            else root.controller.terminal_action("close",tabDelegate.modelData.id,false)
                        }
                    }
                }
            }
        }
    }
    Dialog {
        id: closeDialog
        width: 420
        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        readonly property var prompt: root.controller.terminal_confirmation()
        title: prompt.title
        contentItem: Label { text: closeDialog.prompt.body; wrapMode: Text.WordWrap; width: 360 }
        footer: DialogButtonBox {
            Button { text: closeDialog.prompt.cancel; DialogButtonBox.buttonRole: DialogButtonBox.RejectRole }
            Button { text: closeDialog.prompt.confirm; DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole }
            onAccepted: closeDialog.accept()
            onRejected: closeDialog.reject()
        }
        onAccepted: { root.controller.terminal_action("close",root.closing,true); root.closing=null }
        onRejected: root.closing=null
    }
}
