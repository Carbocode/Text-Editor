import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
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
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: tabs.implicitHeight
                Row {
                    id: tabs
                    Repeater {
                        model: root.tabsModel
                        Row {
                            required property var modelData
                            ToolButton { text: modelData.title; checked: root.state.active === modelData.id; checkable: true; onClicked: root.controller.terminal_action("activate",modelData.id,false) }
                            ToolButton { text: "×"; Accessible.name: "Close terminal"; onClicked: {
                                if (modelData.running) { root.closing=modelData.id; closeDialog.open() }
                                else root.controller.terminal_action("close",modelData.id,false)
                            } }
                        }
                    }
                }
            }
            ToolButton { text: "+"; Accessible.name: "New terminal"; onClicked: root.controller.terminal_action("new",0,false) }
            ToolButton { text: "⌄"; Accessible.name: "Hide terminal"; onClicked: root.visible=false }
        }
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
    Dialog {
        id: closeDialog
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
