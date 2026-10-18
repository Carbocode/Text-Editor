import QtQuick
import QtQuick.Controls
import org.nativeworkspace.Diagnostic
import org.nativeworkspace.Test

ApplicationWindow {
    id: window
    visible: true
    width: 800
    height: 360
    AppController { id: controller }
    TerminalPanel { id: panel; anchors.fill: parent; controller: controller }
    property int stage: 0
    property var first: null
    property var second: null
    function check(value,message) { if (!value) throw new Error(message) }
    function text(state) { return state.frame ? state.frame.cells.map(c => c.text).join("") : "" }
    function find(item,name) {
        if (item.objectName===name) return item
        for (const child of item.children) { const found=find(child,name); if(found) return found }
        return null
    }
    Component.onCompleted: {
        const argument=Qt.application.arguments.find(a => a.startsWith("--smoke-terminal="))
        controller.open_workspace(argument.slice(17))
    }
    Timer {
        interval: 16
        running: true
        repeat: true
        property int ticks: 0
        onTriggered: {
            try {
                check(++ticks<900,"Terminal smoke timed out")
                controller.poll()
                if(controller.move_pending) return
                const state=controller.terminal_json.length ? JSON.parse(controller.terminal_json) : {tabs:[]}
                if(stage===0) { controller.terminal_action("new",0,false);stage=1 }
                else if(stage===1 && state.frame) {
                    first=state.active
                    check(find(panel,"terminalSurface").width>panel.width/2,"Tab column covers the terminal")
                    TerminalProbe.type(find(panel,"terminalSurface"),"printf 'NATIVE_%s\\n' TERMINAL")
                    stage=2
                } else if(stage===2 && text(state).includes("NATIVE_TERMINAL")) {
                    controller.terminal_action("new",0,false);stage=3
                } else if(stage===3 && state.tabs.length===2 && state.frame) {
                    second=state.active
                    check(second!==first,"Terminals share identity")
                    controller.terminal_action("activate",first,false);stage=4
                } else if(stage===4 && state.active===first) {
                    check(text(state).includes("NATIVE_TERMINAL"),"Switching lost screen")
                    width=500;stage=5
                } else if(stage===5 && state.frame && state.frame.cols<70) {
                    controller.terminal_action("close",second,false)
                    check(controller.error_message.length===0,"Closing an idle shell reported an error")
                    stage=6
                } else if(stage===6 && state.tabs.length===1) {
                    controller.terminal_action("close_all",0,true);stage=7
                } else if(stage===7 && !controller.terminals_running) {
                    check(state.tabs.length===0,"Terminal tabs survived close")
                    console.log("KDE native terminal input/render/resize/tab isolation/termination smoke passed")
                    Qt.quit()
                }
            } catch(error) { console.error(error);Qt.exit(1) }
        }
    }
}
