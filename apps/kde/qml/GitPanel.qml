import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

SplitView {
    id: panel
    required property var controller
    readonly property var gitState: controller.git_json.length ? JSON.parse(controller.git_json) : ({loaded:false,actions:[],history:[]})
    readonly property var colors: ["#5599ee", "#dd9345", "#aa77dd", "#55aa77", "#dd7799", "#55aaaa"]
    property string requestedPath: ""
    property bool requestedStaged: false
    property var selectedLines: []
    property var actionSpec: null
    property string conflictKey: ""
    onGitStateChanged: { const c=gitState.inspection?.conflict; if(c && conflictKey!==c.path+c.expected){conflictKey=c.path+c.expected;mergeDraft.text=c.expected} }
    property string directAction: ""
    property string directValue: ""
    orientation: Qt.Vertical
    function act(action,value,extra,flag) { controller.git_request(action,value ?? "",extra ?? "",flag ?? false) }
    function openAction(action) { actionSpec=action; first.text=""; second.text=""; option.checked=false; actionDialog.open() }
    function review(path,staged) { requestedPath=path;requestedStaged=staged;selectedLines=[];controller.show_git_diff(path,staged);diffDialog.open() }
    function inspect(action,value) { selectedLines=[];act(action,value);diffDialog.open() }
    function confirm(action,value) { directAction=action;directValue=value ?? "";confirmDialog.open() }
    function changeRows() {
        const changes=gitState.repository?.changes ?? []
        let rows=[{header:true,title:"Staged Changes",staged:true}]
        rows=rows.concat(changes.filter(c=>c.staged).map(c=>({header:false,change:c,staged:true})))
        rows.push({header:true,title:"Changes",staged:false})
        return rows.concat(changes.filter(c=>c.unstaged).map(c=>({header:false,change:c,staged:false})))
    }
    ColumnLayout {
        SplitView.minimumHeight: 160
        SplitView.preferredHeight: panel.height/2
        spacing: 4
        ComboBox { Layout.fillWidth:true;visible:(panel.gitState.roots?.length ?? 0)>1;model:panel.gitState.roots ?? [];currentIndex:model.indexOf(panel.gitState.selected_root);onActivated:panel.act("select_repository",model[index]) }
        RowLayout {
            Layout.fillWidth:true
            ComboBox {
                Layout.fillWidth:true; model:panel.gitState.repository?.branches ?? [];textRole:"name"
                displayText:panel.gitState.repository?.branch ?? "Source Control"
                enabled:!panel.controller.move_pending
                onActivated:panel.act("checkout",model[index].name)
            }
            ToolButton { objectName:"refreshGit";icon.name:"view-refresh";Accessible.name:"Refresh Git changes";enabled:!panel.controller.move_pending;onClicked:panel.controller.refresh_git() }
            ToolButton { icon.name:"application-menu";Accessible.name:"Git Actions";onClicked:actionsMenu.popup() }
            Menu {
                id:actionsMenu
                Menu {
                    title:"Stashes"
                    Repeater {model:panel.gitState.repository?.stashes ?? [];MenuItem {required property var modelData;text:"["+modelData.index+"] "+modelData.subject;onTriggered:{panel.openAction(panel.gitState.actions.find(a=>a.id==="stash_apply"));first.text=String(modelData.index)}}}
                }
                Menu {title:"Remotes";Repeater {model:panel.gitState.repository?.remotes ?? [];MenuItem {required property var modelData;text:modelData.name+" — "+modelData.url;onTriggered:panel.act("fetch",modelData.name)}}}
                Menu {title:"Worktrees";Repeater {model:panel.gitState.repository?.worktrees ?? [];MenuItem {required property var modelData;text:modelData.branch+" — "+modelData.path;onTriggered:panel.controller.open_workspace(modelData.path)}}}
                MenuSeparator {}
                Repeater { model:panel.gitState.actions ?? [];MenuItem { required property var modelData;text:modelData.title;enabled:!panel.controller.move_pending;onTriggered:panel.openAction(modelData) } }
            }
        }
        RowLayout {
            visible:!!panel.gitState.repository
            Label { Layout.fillWidth:true;text:"↑ "+(panel.gitState.repository?.ahead ?? 0)+"  ↓ "+(panel.gitState.repository?.behind ?? 0);opacity:0.7 }
            ToolButton { icon.name:"view-refresh";text:"Sync";enabled:!panel.controller.move_pending;onClicked:panel.act("sync") }
            BusyIndicator { running:panel.controller.move_pending;visible:running;Layout.preferredWidth:24;Layout.preferredHeight:24 }
        }
        TextArea { id:message;objectName:"gitCommitMessage";Layout.fillWidth:true;Layout.preferredHeight:64;placeholderText:"Commit message";wrapMode:TextEdit.Wrap;visible:!!panel.gitState.repository }
        RowLayout {
            visible:!!panel.gitState.repository
            CheckBox { id:amend;text:"Amend" }
            CheckBox { id:signoff;text:"Sign off" }
            Button { text:"Commit";enabled:message.text.trim().length>0&&!panel.controller.move_pending;onClicked:panel.act("commit",message.text,signoff.checked?"signoff":"",amend.checked) }
        }
        RowLayout {
            visible:!!panel.gitState.repository?.operation
            Label { Layout.fillWidth:true;text:(panel.gitState.repository?.operation ?? "")+" in progress" }
            Button { text:"Continue";enabled:!panel.controller.move_pending;onClicked:panel.act("continue") }
            Button { text:"Abort";enabled:!panel.controller.move_pending;onClicked:panel.confirm("abort") }
        }
        Label { Layout.fillWidth:true;visible:!panel.gitState.repository;text:"Initialize or clone a repository from Git Actions.";wrapMode:Text.Wrap }
        ListView {
            id:changesList
            Layout.fillWidth:true;Layout.fillHeight:true;clip:true
            model:panel.changeRows()
            delegate:RowLayout {
                required property var modelData
                width:changesList.width; height:32
                Label { Layout.fillWidth:true;visible:modelData.header;text:modelData.title ?? "";font.bold:true }
                Image { visible:!modelData.header;source:modelData.change?panel.controller.file_icon(modelData.change.path):"";Layout.preferredWidth:16;Layout.preferredHeight:16 }
                ToolButton {
                    Layout.fillWidth:true;visible:!modelData.header;text:modelData.change?.path ?? "";display:AbstractButton.TextOnly
                    enabled:!panel.controller.move_pending
                    onClicked:panel.review(modelData.change.path,modelData.staged)
                    onPressAndHold:fileMenu.popup()
                    Menu {
                        id:fileMenu
                        MenuItem { text:"Resolve in Merge Editor";visible:modelData.change?.conflicted ?? false;onTriggered:panel.inspect("resolve",modelData.change.path) }
                        MenuItem { text:"File History";onTriggered:panel.inspect("file_history",modelData.change.path) }
                        MenuItem { text:"Blame";onTriggered:panel.inspect("blame",modelData.change.path) }
                        MenuItem { text:"Ignore";onTriggered:panel.act("ignore",modelData.change.path) }
                        MenuItem { text:"Discard Changes…";enabled:!modelData.staged;onTriggered:panel.confirm("discard",modelData.change.path) }
                    }
                    TapHandler { acceptedButtons:Qt.RightButton;onTapped:fileMenu.popup() }
                }
                Label { visible:!modelData.header;text:modelData.change?.conflicted?"!":(modelData.staged?modelData.change?.index:modelData.change?.worktree) ?? "" }
                ToolButton {
                    icon.name:modelData.staged?"list-remove":"list-add";enabled:!panel.controller.move_pending
                    Accessible.name:modelData.staged?(modelData.header?"Unstage All":"Unstage"):(modelData.header?"Stage All":"Stage")
                    onClicked:panel.act(modelData.staged?"unstage":"stage",modelData.header?"":modelData.change.path)
                }
            }
            ScrollBar.vertical:ScrollBar {}
        }
        Label { Layout.fillWidth:true;visible:panel.controller.error_message.length>0;text:panel.controller.error_message;wrapMode:Text.Wrap;maximumLineCount:4;color:Kirigami.Theme.negativeTextColor }
    }
    ColumnLayout {
        objectName:"gitGraph"
        SplitView.minimumHeight:160
        SplitView.preferredHeight:panel.height/2
        Label { Layout.fillWidth:true;text:"Source Control Graph";font.bold:true }
        ListView {
            id:history
            Layout.fillWidth:true;Layout.fillHeight:true;clip:true;model:panel.gitState.history ?? []
            delegate:ItemDelegate {
                id:commitRow
                required property var modelData
                width:history.width;height:54;padding:0
                onClicked:panel.inspect("show_commit",modelData.commit.id)
                onPressAndHold:commitMenu.popup()
                TapHandler { acceptedButtons:Qt.RightButton;onTapped:commitMenu.popup() }
                contentItem:RowLayout {
                    Canvas {
                        id:graphCanvas
                        Layout.preferredWidth:Math.max(2,commitRow.modelData.columns)*14;Layout.fillHeight:true
                        onPaint:{
                            const ctx=getContext("2d");ctx.reset();ctx.lineWidth=2
                            for(const edge of commitRow.modelData.edges){
                                const ax=edge.from*14+7,ay=edge.start*height/2,bx=edge.to*14+7,by=edge.end*height/2
                                ctx.strokeStyle=panel.colors[edge.color%panel.colors.length];ctx.beginPath();ctx.moveTo(ax,ay);ctx.bezierCurveTo(ax,(ay+by)/2,bx,(ay+by)/2,bx,by);ctx.stroke()
                            }
                            ctx.fillStyle=panel.colors[commitRow.modelData.lane%panel.colors.length];ctx.beginPath();ctx.arc(commitRow.modelData.lane*14+7,height/2,4,0,Math.PI*2);ctx.fill()
                        }
                        Connections { target:commitRow;function onModelDataChanged(){graphCanvas.requestPaint()} }
                    }
                    ColumnLayout {
                        Layout.fillWidth:true;spacing:0
                        Label { Layout.fillWidth:true;visible:text.length>0;text:commitRow.modelData.commit.refs;font.bold:true;font.pixelSize:10;elide:Text.ElideRight;color:Kirigami.Theme.linkColor }
                        Label { Layout.fillWidth:true;text:commitRow.modelData.commit.subject;elide:Text.ElideRight }
                        Label { Layout.fillWidth:true;text:commitRow.modelData.commit.id.slice(0,7)+" · "+commitRow.modelData.commit.author+" · "+commitRow.modelData.commit.date.slice(0,10);font.pixelSize:10;opacity:0.7;elide:Text.ElideRight }
                    }
                }
                Menu {
                    id:commitMenu
                    MenuItem { text:"Show Commit";onTriggered:panel.inspect("show_commit",commitRow.modelData.commit.id) }
                    MenuItem { text:"Checkout Commit";onTriggered:panel.act("checkout",commitRow.modelData.commit.id) }
                    MenuItem { text:"Cherry-pick";onTriggered:panel.act("cherry_pick",commitRow.modelData.commit.id) }
                    MenuItem { text:"Revert Commit";onTriggered:panel.act("revert",commitRow.modelData.commit.id) }
                    MenuItem { text:"Create Branch Here…";onTriggered:{panel.openAction(panel.gitState.actions.find(a=>a.id==="branch"));second.text=commitRow.modelData.commit.id} }
                }
            }
            footer:Button { text:"Load More Commits";visible:history.count>=(panel.gitState.history_limit ?? 200);enabled:!panel.controller.move_pending;onClicked:panel.controller.git_load_more() }
            ScrollBar.vertical:ScrollBar {}
        }
    }
    Dialog {
        id:actionDialog;parent:Overlay.overlay;anchors.centerIn:parent;modal:true
        width:Math.min(460,parent?parent.width-40:460);title:panel.actionSpec?.title ?? "Git Action"
        standardButtons:Dialog.Ok|Dialog.Cancel
        onAccepted:panel.act(panel.actionSpec.id,first.text,second.text,option.checked)
        ColumnLayout {
            anchors.fill:parent
            TextField { id:first;Layout.fillWidth:true;visible:(panel.actionSpec?.first ?? "").length>0;placeholderText:panel.actionSpec?.first ?? "" }
            TextField { id:second;Layout.fillWidth:true;visible:(panel.actionSpec?.second ?? "").length>0;placeholderText:panel.actionSpec?.second ?? "" }
            CheckBox { id:option;visible:(panel.actionSpec?.option ?? "").length>0;text:panel.actionSpec?.option ?? "" }
            Label { Layout.fillWidth:true;visible:panel.actionSpec?.destructive ?? false;text:panel.controller.git_confirmation().body;wrapMode:Text.Wrap }
        }
    }
    Dialog {
        id:confirmDialog;parent:Overlay.overlay;anchors.centerIn:parent;modal:true
        title:panel.controller.git_confirmation().title
        standardButtons:Dialog.Ok|Dialog.Cancel
        onAccepted:panel.act(panel.directAction,panel.directValue)
        Label { width:360;text:panel.controller.git_confirmation().body;wrapMode:Text.Wrap }
    }
    Dialog {
        id:diffDialog;objectName:"gitDiffDialog";parent:Overlay.overlay;anchors.centerIn:parent
        width:Math.min(1100,parent?parent.width-40:1000);height:Math.min(750,parent?parent.height-40:650)
        modal:true;standardButtons:Dialog.Close
        title:panel.gitState.inspection?.title ?? panel.gitState.diff?.path ?? "Git Changes"
        ColumnLayout {
            anchors.fill:parent
            CheckBox { id:sideBySide;objectName:"gitDiffMode";text:"Side by Side";checked:true;visible:!!panel.gitState.diff }
            SplitView {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:sideBySide.checked && panel.gitState.diff?.before !== null && panel.gitState.diff?.after !== null && !!panel.gitState.diff && !panel.controller.move_pending
                ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;selectByMouse:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.diff?.before ?? "" } }
                ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;selectByMouse:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.diff?.after ?? "" } }
            }
            Button { visible:!!panel.gitState.diff;enabled:panel.selectedLines.length>0&&!panel.controller.move_pending;text:panel.gitState.diff?.staged?"Unstage Selected Lines":"Stage Selected Lines";onClicked:{panel.controller.git_stage_lines(JSON.stringify(panel.selectedLines));diffDialog.close()} }
            ColumnLayout {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:!!panel.gitState.inspection?.conflict
                SplitView {
                    Layout.fillWidth:true;Layout.preferredHeight:180
                    ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.inspection?.conflict?.current ?? "" } }
                    ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.inspection?.conflict?.incoming ?? "" } }
                }
                RowLayout {
                    Button { text:"Accept Current";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_current }
                    Button { text:"Accept Incoming";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_incoming }
                    Button { text:"Accept Both";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_both }
                    Button { text:"Save and Stage Result";enabled:!panel.controller.move_pending;onClicked:{panel.controller.git_resolve(mergeDraft.text);diffDialog.close()} }
                }
                ScrollView { Layout.fillWidth:true;Layout.fillHeight:true;TextArea { id:mergeDraft;font.family:"monospace";textFormat:TextEdit.PlainText;selectByMouse:true } }
            }
            SplitView {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:!panel.gitState.inspection?.conflict && (!sideBySide.checked || !panel.gitState.diff || panel.gitState.diff.before === null || panel.gitState.diff.after === null)
                ScrollView {
                    SplitView.fillWidth:true
                    TextArea {
                        objectName:"gitDiffText";readOnly:true;selectByMouse:true;wrapMode:TextEdit.NoWrap;textFormat:TextEdit.PlainText;font.family:"monospace"
                        text:panel.controller.move_pending?"Loading…":panel.controller.error_message.length?panel.controller.error_message:panel.gitState.inspection?.text ?? panel.gitState.diff?.text ?? ""
                    }
                }
                ListView {
                    objectName:"gitPatchLines";SplitView.preferredWidth:300;visible:!!panel.gitState.diff;clip:true
                    model:(panel.gitState.patch_lines ?? []).filter(line=>line.changed)
                    delegate:CheckDelegate {
                        required property var modelData
                        width:ListView.view.width;text:modelData.text.replace(/\n$/, "");font.family:"monospace"
                        checked:panel.selectedLines.includes(modelData.index)
                        onToggled:{let copy=panel.selectedLines.slice();if(checked&&!copy.includes(modelData.index))copy.push(modelData.index);if(!checked)copy=copy.filter(i=>i!==modelData.index);panel.selectedLines=copy}
                    }
                }
            }
        }
    }
}
