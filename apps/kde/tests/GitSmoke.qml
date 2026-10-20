import QtQuick
import QtQuick.Controls
import org.nativeworkspace.Diagnostic

ApplicationWindow {
    id: window
    visible: true
    width: 900
    height: 650
    AppController { id: controller }
    // Narrowest sidebar width: rows must elide instead of overflowing.
    GitPanel { id: panel; anchors.top: parent.top; anchors.bottom: parent.bottom; width: 260; controller: controller }
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
                    check(controller.sidebar_git_status("note.txt") === "M", "Files decoration missing")
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
                else if(stage===5) {panel.act("commit","Native commit with a deliberately long subject that must elide in the sidebar");stage=6}
                else if(stage===6) {panel.act("tag","release-candidate-with-a-very-long-tag-name-0001","HEAD");stage=7}
                else if(stage===7) {panel.act("tag","v2","HEAD");stage=8}
                else if(stage===8) {
                    const rows=[]
                    function collect(item){ if(item.objectName==="gitCommitRow") rows.push(item); for(const c of item.children) collect(c) }
                    collect(panel)
                    check(rows.length>0,"Commit rows missing")
                    for(const row of rows) check(row.childrenRect.x>=0&&row.childrenRect.x+row.childrenRect.width<=row.width+1,"Commit row overflows: "+row.childrenRect.width+" > "+row.width)
                    stage=9
                }
                else {
                    check(state.history[0].commit.subject.startsWith("Native commit"),"Native commit did not update graph")
                    check(state.repository.changes.length===0,"Commit left staged changes")
                    check(controller.sidebar_git_status("note.txt") === "", "Files decoration stayed stale after commit")
                    check(state.history[0].head && state.history[0].commit.refs.some(r=>r.current&&r.kind==="branch") && state.history[0].commit.refs.length===3,"Reference badges missing")
                    check(state.graph_width===1,"Linear history should use one lane")
                    console.log("KDE Git status and staged/working-tree diff smoke passed")
                    Qt.quit()
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
