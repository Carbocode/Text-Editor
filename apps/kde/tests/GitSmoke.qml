import QtQuick
import QtQuick.Controls
import org.nativeworkspace.Diagnostic

ApplicationWindow {
    id: window
    visible: true
    width: 900
    height: 650
    AppController { id: controller }
    GitPanel { id: panel; anchors.fill: parent; controller: controller }
    property int stage: 0
    function check(value, message) { if (!value) throw new Error(message) }
    function find(item, name) {
        if (item.objectName === name) return item
        for (const child of item.children) {
            const found = find(child, name)
            if (found) return found
        }
        return null
    }
    Component.onCompleted: {
        const argument = Qt.application.arguments.find(a => a.startsWith("--smoke-git="))
        controller.open_workspace(argument.slice(12))
        controller.refresh_git()
    }
    Timer {
        interval: 16
        running: true
        repeat: true
        property int ticks: 0
        onTriggered: {
            try {
                check(++ticks < 600, "Git smoke timed out")
                controller.poll()
                if (controller.move_pending) return
                check(controller.error_message.length === 0, controller.error_message)
                const state = JSON.parse(controller.git_json)
                if (stage === 0) {
                    const graph=find(panel,"gitGraph")
                    check(graph!==null && Math.abs(graph.height/panel.height-0.5)<0.15,"Git sidebar is not split in half")
                    check(state.loaded && !!state.repository, "Git repository missing")
                    const change = state.repository.changes.find(c => c.path === "note.txt")
                    check(change && change.staged && change.unstaged, "Index/worktree changes missing")
                    panel.review("note.txt", true)
                    stage = 1
                } else if (stage === 1) {
                    const text = find(window.Overlay.overlay, "gitDiffText")
                    check(state.diff.text.includes("+staged"), "Incorrect index diff")
                    check(text !== null && text.text.includes("+staged"), "Native diff text missing")
                    find(window.Overlay.overlay,"gitDiffMode").checked=false
                    stage=10
                } else if(stage===10) {
                    const lines=find(window.Overlay.overlay,"gitPatchLines")
                    const item=lines.itemAtIndex(0)
                    check(item && item.text.length>0,"Partial staging lines did not render")
                    panel.review("note.txt",false);stage=2
                } else if (stage === 2) {
                    check(state.diff.text.includes("+working tree"), "Incorrect working-tree diff")
                    controller.refresh_git()
                    stage = 3
                } else if(stage===3) {
                    check(state.diff === null, "Refresh retained an old diff")
                    panel.act("identity","Native Test","native@example.invalid");stage=4
                } else if(stage===4) {panel.act("stage");stage=5}
                else if(stage===5) {panel.act("commit","Native commit");stage=6}
                else {
                    check(state.history[0].commit.subject==="Native commit","Native commit did not update graph")
                    check(state.repository.changes.length===0,"Commit left staged changes")
                    check(state.history[0].head && state.history[0].commit.refs.some(r=>r.current&&r.kind==="branch"),"Current branch badge missing")
                    check(state.graph_width===1,"Linear history should use one lane")
                    console.log("KDE Git status and staged/working-tree diff smoke passed")
                    Qt.quit()
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
