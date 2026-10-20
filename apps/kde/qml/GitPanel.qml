import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

SplitView {
    id: panel
    required property var controller
    readonly property var gitState: controller.git_json.length ? JSON.parse(controller.git_json) : ({loaded:false,actions:[],history:[],graph_width:0,patch_lines:[]})
    readonly property var repo: gitState.repository ?? null
    readonly property var colors: ["#3d8ffa", "#f58c33", "#9e6bf0", "#40b878", "#ed5c8f", "#2eb3c2", "#d9b329", "#8c99ad"]
    readonly property int laneWidth: 10
    // Only the first lanes are drawn so the gutter cannot push row text out of the sidebar.
    readonly property int maxLanes: 8
    readonly property int gutter: Math.max(1, Math.min(gitState.graph_width ?? 1, maxLanes)) * laneWidth + 4
    readonly property bool busy: controller.move_pending
    property var selectedLines: []
    property var actionSpec: null
    property string conflictKey: ""
    property string selectedCommit: ""
    property bool committing: false
    property string lastHead: ""
    property string directAction: ""
    property string directValue: ""
    orientation: Qt.Vertical
    /// Reference names elide in the middle when the badge is narrower than its text.
    component RefBadge: Rectangle {
        property var ref:null
        property int lane:0
        readonly property color tint:ref?.kind==="tag"?Kirigami.Theme.neutralTextColor:ref?.kind==="head"?Kirigami.Theme.negativeTextColor:panel.colors[lane%panel.colors.length]
        implicitWidth:refLabel.implicitWidth+10;implicitHeight:16;radius:4
        color:ref?.current?tint:Qt.alpha(tint,0.16);border.width:ref?.kind==="remote"?1:0;border.color:Qt.alpha(tint,0.6)
        Label {
            id:refLabel;anchors.verticalCenter:parent.verticalCenter;x:5;width:parent.width-10
            font.pixelSize:10;font.bold:true;elide:Text.ElideMiddle
            text:ref?((ref.kind==="tag"?"🏷 ":ref.kind==="remote"?"☁ ":ref.current?"✓ ":"")+ref.name):""
            color:ref?.current?"white":parent.tint
        }
        HoverHandler { id:badgeHover }
        ToolTip.visible:badgeHover.hovered;ToolTip.text:ref?.name ?? ""
        TapHandler { acceptedButtons:Qt.RightButton;onTapped:{refMenu.target=parent.ref;refMenu.popup()} }
    }
    onGitStateChanged: {
        const c=gitState.inspection?.conflict
        if(c && conflictKey!==c.path+c.expected){conflictKey=c.path+c.expected;mergeDraft.text=c.expected}
        // A commit draft is cleared only once HEAD moves.
        const head=repo?.head ?? ""
        if(committing && head!==lastHead){committing=false;message.text="";amend.checked=false}
        lastHead=head
    }
    function act(action,value,extra,flag) { controller.git_request(action,value ?? "",extra ?? "",flag ?? false) }
    function spec(id) { return gitState.actions.find(a=>a.id===id) }
    function openAction(action,prefill) {
        if(!action.first && !action.second && !action.option){ if(action.destructive) confirm(action.id); else act(action.id); return }
        actionSpec=action;first.text="";second.text="";option.checked=false
        if(prefill){ if(action.id==="branch"||action.id==="tag") second.text=prefill; else first.text=prefill }
        actionDialog.open()
    }
    function review(path,staged) { selectedLines=[];controller.show_git_diff(path,staged);diffDialog.open() }
    function inspect(action,value,extra) { selectedLines=[];act(action,value,extra);diffDialog.open() }
    function confirm(action,value) { directAction=action;directValue=value ?? "";confirmDialog.open() }
    function status(code) {
        return code==="?"?["U","positive"]:code==="A"?["A","positive"]:code==="D"?["D","negative"]:(code==="R"||code==="C")?[code,"link"]:[code,"neutral"]
    }
    function changeStatus(change,staged) { return change.conflicted?["!","negative"]:status(staged?change.index:change.worktree) }
    function tone(name) { return name==="positive"?Kirigami.Theme.positiveTextColor:name==="negative"?Kirigami.Theme.negativeTextColor:name==="link"?Kirigami.Theme.linkColor:Kirigami.Theme.neutralTextColor }
    function changeRows() {
        const changes=repo?.changes ?? []
        let rows=[]
        for (const [title,staged,include] of [["Merge Changes",false,c=>c.conflicted],["Staged Changes",true,c=>c.staged&&!c.conflicted],["Changes",false,c=>c.unstaged&&!c.conflicted]]) {
            const items=changes.filter(include)
            if(!items.length && title!=="Changes") continue
            rows.push({header:true,title:title,staged:staged,count:items.length})
            rows=rows.concat(items.map(c=>({header:false,change:c,staged:staged})))
        }
        return rows
    }
    function actionRows() {
        let rows=[],group=""
        for(const a of gitState.actions ?? []){ if(a.group!==group){group=a.group;rows.push({header:true,title:group})} rows.push({header:false,action:a}) }
        return rows
    }
    function matches(row) {
        const f=filter.text.toLowerCase()
        return !f || [row.commit.subject,row.commit.author,row.commit.id].concat(row.commit.refs.map(r=>r.name)).some(v=>v.toLowerCase().includes(f))
    }
    function toggleCommit(id) {
        if(selectedCommit===id){selectedCommit="";return}
        selectedCommit=id
        if(gitState.inspection?.commit?.id!==id) act("show_commit",id)
    }
    function openFile(path) { if(repo) controller.open_paths([repo.root+"/"+path]) }

    // MARK: changes pane
    ColumnLayout {
        SplitView.minimumHeight: 160
        SplitView.preferredHeight: panel.height/2
        spacing: 4
        ComboBox { Layout.fillWidth:true;visible:(panel.gitState.roots?.length ?? 0)>1;model:panel.gitState.roots ?? [];currentIndex:model.indexOf(panel.gitState.selected_root);onActivated:panel.act("select_repository",model[index]) }
        RowLayout {
            Layout.fillWidth:true;spacing:2
            ToolButton {
                Layout.fillWidth:true;icon.name:"vcs-branch";text:panel.repo?.branch ?? "Source Control";enabled:!panel.busy
                contentItem:Label{text:parent.text;elide:Text.ElideMiddle;verticalAlignment:Text.AlignVCenter;leftPadding:20}
                onClicked:branchMenu.popup()
                ToolTip.visible:hovered;ToolTip.text:panel.repo?(panel.repo.branch+" — "+panel.repo.root):""
                Menu {
                    id:branchMenu
                    Repeater { model:(panel.repo?.branches ?? []).filter(b=>!b.remote);MenuItem { required property var modelData;text:(modelData.current?"✓ ":"   ")+modelData.name;enabled:!modelData.current;onTriggered:panel.act("checkout",modelData.name) } }
                    MenuSeparator {}
                    Menu { title:"Remote Branches";Repeater { model:(panel.repo?.branches ?? []).filter(b=>b.remote);MenuItem { required property var modelData;text:modelData.name;onTriggered:panel.act("checkout",modelData.name) } } }
                    MenuItem { text:"New Branch…";onTriggered:panel.openAction(panel.spec("branch")) }
                    MenuItem { text:"Checkout Branch or Commit…";onTriggered:panel.openAction(panel.spec("checkout")) }
                }
            }
            BusyIndicator { running:panel.busy;visible:running;Layout.preferredWidth:20;Layout.preferredHeight:20 }
            ToolButton {
                visible:(panel.repo?.ahead ?? 0)+(panel.repo?.behind ?? 0)>0;enabled:!panel.busy
                text:((panel.repo?.behind ?? 0)>0?"↓"+panel.repo.behind+" ":"")+((panel.repo?.ahead ?? 0)>0?"↑"+panel.repo.ahead:"")
                ToolTip.visible:hovered;ToolTip.text:"Sync"
                onClicked:panel.act("sync")
            }
            ToolButton { objectName:"refreshGit";icon.name:"view-refresh";Accessible.name:"Refresh Git changes";enabled:!panel.busy;onClicked:panel.controller.refresh_git();ToolTip.visible:hovered;ToolTip.text:"Refresh" }
            ToolButton { icon.name:"application-menu";Accessible.name:"Git Actions";enabled:!panel.busy;onClicked:actionsMenu.popup() }
            Menu {
                id:actionsMenu
                MenuItem { text:"Fetch";onTriggered:panel.act("fetch") }
                MenuItem { text:"Pull";onTriggered:panel.act("pull") }
                MenuItem { text:"Pull (Rebase)";onTriggered:panel.act("pull","","",true) }
                MenuItem { text:"Push";onTriggered:panel.act("push") }
                MenuSeparator {}
                Menu {
                    title:"Stashes"
                    MenuItem { text:"Stash All Changes…";onTriggered:panel.openAction(panel.spec("stash")) }
                    Repeater {model:panel.repo?.stashes ?? [];MenuItem {required property var modelData;text:modelData.index+": "+modelData.subject;onTriggered:panel.openAction(panel.spec("stash_apply"),String(modelData.index))}}
                }
                Menu {title:"Remotes";Repeater {model:panel.repo?.remotes ?? [];MenuItem {required property var modelData;text:"Fetch "+modelData.name+" — "+modelData.url;onTriggered:panel.act("fetch",modelData.name)}}}
                Menu {title:"Worktrees";Repeater {model:panel.repo?.worktrees ?? [];MenuItem {required property var modelData;text:modelData.branch+" — "+modelData.path;onTriggered:panel.controller.open_workspace(modelData.path)}}}
                MenuSeparator {}
                Repeater {
                    model:panel.actionRows()
                    MenuItem {
                        required property var modelData
                        text:modelData.header?modelData.title.toUpperCase():"   "+modelData.action.title
                        enabled:!modelData.header;font.bold:modelData.header
                        onTriggered:panel.openAction(modelData.action)
                    }
                }
            }
        }
        Rectangle {
            Layout.fillWidth:true;visible:!!panel.repo?.operation;implicitHeight:opRow.implicitHeight+8;radius:4
            color:Qt.alpha(Kirigami.Theme.neutralTextColor,0.15)
            RowLayout {
                id:opRow;anchors.fill:parent;anchors.margins:4
                Label { Layout.fillWidth:true;text:(panel.repo?.operation ?? "")+" in progress";font.bold:true;elide:Text.ElideRight }
                Button { text:"Continue";enabled:!panel.busy;onClicked:panel.act("continue") }
                Button { text:"Abort";enabled:!panel.busy;onClicked:panel.confirm("abort") }
            }
        }
        TextArea {
            id:message;objectName:"gitCommitMessage";Layout.fillWidth:true;Layout.preferredHeight:Math.max(34,Math.min(110,contentHeight+14))
            placeholderText:"Message (Ctrl+Return to commit)";wrapMode:TextEdit.Wrap;visible:!!panel.repo
            Keys.onPressed:(event)=>{ if((event.key===Qt.Key_Return||event.key===Qt.Key_Enter)&&(event.modifiers&Qt.ControlModifier)){ if(commitButton.enabled) commitButton.clicked(); event.accepted=true } }
        }
        // Stacked so the commit controls fit the narrowest sidebar.
        ColumnLayout {
            visible:!!panel.repo;spacing:2;Layout.fillWidth:true
            readonly property int staged:(panel.repo?.changes ?? []).filter(c=>c.staged).length
            Button {
                id:commitButton;Layout.fillWidth:true;highlighted:true
                text:amend.checked?"Amend Commit":parent.staged>0?"Commit "+parent.staged+(parent.staged===1?" File":" Files"):"Commit"
                enabled:message.text.trim().length>0&&(parent.staged>0||amend.checked)&&!panel.busy
                ToolTip.visible:hovered&&!enabled&&parent.staged===0;ToolTip.text:"Stage changes to commit"
                onClicked:{panel.committing=true;panel.act("commit",message.text,signoff.checked?"signoff":"",amend.checked)}
            }
            RowLayout {
                CheckBox { id:amend;text:"Amend" }
                CheckBox { id:signoff;text:"Sign-off" }
            }
        }
        Label { Layout.fillWidth:true;visible:!panel.repo;text:"This folder is not a Git repository.";wrapMode:Text.Wrap;opacity:0.7 }
        Flow {
            visible:!panel.repo;Layout.fillWidth:true;spacing:4
            Button { text:"Initialize Repository…";highlighted:true;onClicked:panel.openAction(panel.spec("init")) }
            Button { text:"Clone…";onClicked:panel.openAction(panel.spec("clone")) }
        }
        ListView {
            id:changesList
            Layout.fillWidth:true;Layout.fillHeight:true;clip:true;visible:!!panel.repo
            model:panel.changeRows()
            delegate:ItemDelegate {
                id:changeRow
                required property var modelData
                width:changesList.width;height:modelData.header?26:24;padding:0;enabled:!panel.busy
                readonly property var info:modelData.header?["",""]:panel.changeStatus(modelData.change,modelData.staged)
                onClicked:{ if(modelData.header) return; if(modelData.change.conflicted) panel.inspect("resolve",modelData.change.path); else panel.review(modelData.change.path,modelData.staged) }
                onDoubleClicked:if(!modelData.header) panel.openFile(modelData.change.path)
                TapHandler { acceptedButtons:Qt.RightButton;onTapped:if(!changeRow.modelData.header) fileMenu.popup() }
                ToolTip.visible:hovered&&!modelData.header;ToolTip.delay:600;ToolTip.text:modelData.change?.path ?? ""
                contentItem:RowLayout {
                    spacing:5
                    Label { visible:changeRow.modelData.header;Layout.fillWidth:true;Layout.leftMargin:2;text:(changeRow.modelData.title ?? "").toUpperCase();font.bold:true;font.pixelSize:10;opacity:0.65 }
                    ToolButton {
                        visible:changeRow.modelData.header&&changeRow.modelData.title!=="Merge Changes"&&changeRow.modelData.count>0
                        icon.name:changeRow.modelData.staged?"list-remove":"list-add";implicitWidth:22;implicitHeight:22
                        Accessible.name:changeRow.modelData.staged?"Unstage All":"Stage All";ToolTip.visible:hovered;ToolTip.text:Accessible.name
                        onClicked:panel.act(changeRow.modelData.staged?"unstage":"stage")
                    }
                    Label { visible:changeRow.modelData.header;text:changeRow.modelData.count ?? "";font.pixelSize:10;opacity:0.65;Layout.rightMargin:6 }
                    Image { visible:!changeRow.modelData.header;Layout.leftMargin:14;source:changeRow.modelData.change?panel.controller.file_icon(changeRow.modelData.change.path):"";Layout.preferredWidth:14;Layout.preferredHeight:14 }
                    Label { visible:!changeRow.modelData.header;text:changeRow.modelData.change?panel.controller.path_name(changeRow.modelData.change.path):"";font.strikeout:changeRow.info[0]==="D";elide:Text.ElideMiddle;Layout.maximumWidth:changeRow.width*0.6 }
                    Label { visible:!changeRow.modelData.header;Layout.fillWidth:true;text:changeRow.modelData.change?panel.controller.path_parent(changeRow.modelData.change.path):"";elide:Text.ElideLeft;opacity:0.55;font.pixelSize:11 }
                    ToolButton {
                        visible:!changeRow.modelData.header&&changeRow.hovered&&!changeRow.modelData.staged&&!(changeRow.modelData.change?.conflicted ?? false)
                        icon.name:"edit-undo";implicitWidth:22;implicitHeight:22;Accessible.name:"Discard Changes";ToolTip.visible:hovered;ToolTip.text:Accessible.name
                        onClicked:panel.confirm("discard",changeRow.modelData.change.path)
                    }
                    ToolButton {
                        visible:!changeRow.modelData.header&&!(changeRow.modelData.change?.conflicted ?? false)
                        icon.name:changeRow.modelData.staged?"list-remove":"list-add";implicitWidth:22;implicitHeight:22
                        Accessible.name:changeRow.modelData.staged?"Unstage":"Stage";ToolTip.visible:hovered;ToolTip.text:Accessible.name
                        onClicked:panel.act(changeRow.modelData.staged?"unstage":"stage",changeRow.modelData.change.path)
                    }
                    Label { visible:!changeRow.modelData.header;text:changeRow.info[0];font.family:"monospace";font.bold:true;color:panel.tone(changeRow.info[1]);Layout.preferredWidth:12;Layout.rightMargin:4 }
                }
                Menu {
                    id:fileMenu
                    MenuItem { text:"Resolve in Merge Editor…";visible:changeRow.modelData.change?.conflicted ?? false;height:visible?implicitHeight:0;onTriggered:panel.inspect("resolve",changeRow.modelData.change.path) }
                    MenuItem { text:"Open File";onTriggered:panel.openFile(changeRow.modelData.change.path) }
                    MenuItem { text:"File History";onTriggered:panel.inspect("file_history",changeRow.modelData.change.path) }
                    MenuItem { text:"Blame";onTriggered:panel.inspect("blame",changeRow.modelData.change.path) }
                    MenuItem { text:"Add to .gitignore";onTriggered:panel.act("ignore",changeRow.modelData.change.path) }
                    MenuItem { text:"Discard Changes…";enabled:!changeRow.modelData.staged;onTriggered:panel.confirm("discard",changeRow.modelData.change.path) }
                }
            }
            ScrollBar.vertical:ScrollBar {}
        }
        Label { Layout.fillWidth:true;visible:panel.controller.error_message.length>0;text:panel.controller.error_message;wrapMode:Text.WrapAtWordBoundaryOrAnywhere;maximumLineCount:6;elide:Text.ElideRight;color:Kirigami.Theme.negativeTextColor }
    }

    // MARK: graph pane
    ColumnLayout {
        objectName:"gitGraph"
        SplitView.minimumHeight:160
        SplitView.preferredHeight:panel.height/2
        spacing:2
        RowLayout {
            Label { text:"GRAPH";font.bold:true;font.pixelSize:10;opacity:0.65 }
            Label { text:(panel.gitState.history ?? []).length;font.pixelSize:10;opacity:0.45 }
            TextField { id:filter;Layout.fillWidth:true;placeholderText:"Filter" }
        }
        ListView {
            id:history
            Layout.fillWidth:true;Layout.fillHeight:true;clip:true;model:panel.gitState.history ?? []
            delegate:Column {
                id:commitRow
                required property var modelData
                width:history.width
                readonly property bool selected:panel.selectedCommit===modelData.commit.id
                readonly property var detail:panel.gitState.inspection?.commit?.id===modelData.commit.id?panel.gitState.inspection.commit:null
                ItemDelegate {
                    width:parent.width;height:24;padding:0;highlighted:commitRow.selected
                    opacity:panel.matches(commitRow.modelData)?1:0.3
                    onClicked:panel.toggleCommit(commitRow.modelData.commit.id)
                    TapHandler { acceptedButtons:Qt.RightButton;onTapped:commitMenu.popup() }
                    ToolTip.visible:hovered;ToolTip.delay:700
                    ToolTip.text:commitRow.modelData.commit.subject+"\n"+commitRow.modelData.commit.author+" · "+commitRow.modelData.commit.date+"\n"+commitRow.modelData.commit.id
                    contentItem:RowLayout {
                        objectName:"gitCommitRow";spacing:5
                        Canvas {
                            id:graphCanvas
                            Layout.preferredWidth:panel.gutter;Layout.fillHeight:true
                            onPaint:{
                                const ctx=getContext("2d");ctx.reset();ctx.lineWidth=1.6;ctx.lineCap="round"
                                const row=commitRow.modelData,x=l=>l*panel.laneWidth+panel.laneWidth/2+2
                                for(const edge of row.edges){
                                    if(Math.max(edge.from,edge.to)>=panel.maxLanes) continue
                                    const ax=x(edge.from),ay=edge.start*height/2,bx=x(edge.to),by=edge.end*height/2
                                    ctx.strokeStyle=panel.colors[edge.color%panel.colors.length];ctx.beginPath();ctx.moveTo(ax,ay);ctx.bezierCurveTo(ax,(ay+by)/2,bx,(ay+by)/2,bx,by);ctx.stroke()
                                }
                                if(row.lane>=panel.maxLanes) return
                                const color=panel.colors[row.color%panel.colors.length],cx=x(row.lane),cy=height/2,r=row.head?4.5:3.5
                                ctx.beginPath();ctx.arc(cx,cy,r,0,Math.PI*2)
                                if(row.merge){ctx.fillStyle=Kirigami.Theme.backgroundColor;ctx.fill();ctx.strokeStyle=color;ctx.lineWidth=1.8;ctx.stroke()}
                                else {ctx.fillStyle=color;ctx.fill()}
                                if(row.head){ctx.globalAlpha=0.55;ctx.lineWidth=1.2;ctx.strokeStyle=color;ctx.beginPath();ctx.arc(cx,cy,6.5,0,Math.PI*2);ctx.stroke();ctx.globalAlpha=1}
                            }
                            Connections { target:commitRow;function onModelDataChanged(){graphCanvas.requestPaint()} }
                            Connections { target:panel;function onGutterChanged(){graphCanvas.requestPaint()} }
                        }
                        // One badge in the row; the commit detail lists every reference.
                        RefBadge {
                            visible:commitRow.modelData.commit.refs.length>0
                            ref:commitRow.modelData.commit.refs[0] ?? null;lane:commitRow.modelData.color
                            Layout.maximumWidth:Math.min(110,commitRow.width*0.35);Layout.preferredWidth:implicitWidth
                        }
                        Label {
                            visible:commitRow.modelData.commit.refs.length>1;text:"+"+(commitRow.modelData.commit.refs.length-1)
                            font.pixelSize:10;font.bold:true;opacity:0.7;leftPadding:4;rightPadding:4
                            background:Rectangle { radius:4;color:Qt.alpha(Kirigami.Theme.textColor,0.08) }
                            ToolTip.visible:moreHover.hovered;ToolTip.text:commitRow.modelData.commit.refs.map(r=>r.name).join("\n")
                            HoverHandler { id:moreHover }
                        }
                        Label { Layout.fillWidth:true;Layout.minimumWidth:24;text:commitRow.modelData.commit.subject;elide:Text.ElideRight;font.bold:commitRow.modelData.head }
                        Label { text:commitRow.modelData.commit.date.slice(0,10);font.pixelSize:10;opacity:0.6;Layout.rightMargin:6 }
                    }
                }
                // Inline commit summary; lanes continue through it.
                RowLayout {
                    width:parent.width;visible:commitRow.selected;spacing:0
                    Canvas {
                        id:lanes
                        Layout.preferredWidth:panel.gutter;Layout.fillHeight:true
                        onPaint:{
                            const ctx=getContext("2d");ctx.reset();ctx.lineWidth=1.6
                            const seen={}
                            for(const edge of commitRow.modelData.edges){
                                if(edge.end!==2||edge.to>=panel.maxLanes||seen[edge.to]) continue
                                seen[edge.to]=true
                                const x=edge.to*panel.laneWidth+panel.laneWidth/2+2
                                ctx.strokeStyle=panel.colors[edge.color%panel.colors.length];ctx.beginPath();ctx.moveTo(x,0);ctx.lineTo(x,height);ctx.stroke()
                            }
                        }
                        onHeightChanged:requestPaint()
                    }
                    Rectangle {
                        Layout.fillWidth:true;Layout.rightMargin:8;Layout.topMargin:2;Layout.bottomMargin:4
                        implicitHeight:detailColumn.implicitHeight+12;radius:6;color:Qt.alpha(Kirigami.Theme.textColor,0.05)
                        ColumnLayout {
                            id:detailColumn;anchors.fill:parent;anchors.margins:6;spacing:2
                            BusyIndicator { visible:!commitRow.detail;running:visible;Layout.preferredHeight:20 }
                            Flow {
                                Layout.fillWidth:true;spacing:4;visible:!!commitRow.detail&&commitRow.modelData.commit.refs.length>0
                                Repeater { model:commitRow.modelData.commit.refs;RefBadge { required property var modelData;ref:modelData;lane:commitRow.modelData.color;width:Math.min(implicitWidth,parent.width) } }
                            }
                            Label { visible:!!commitRow.detail;Layout.fillWidth:true;text:commitRow.detail?.message ?? "";wrapMode:Text.WrapAtWordBoundaryOrAnywhere }
                            Label {
                                visible:!!commitRow.detail;Layout.fillWidth:true;wrapMode:Text.WrapAtWordBoundaryOrAnywhere;opacity:0.6;font.pixelSize:10
                                text:commitRow.detail?(commitRow.detail.author+" <"+commitRow.detail.email+"> · "+commitRow.detail.date.slice(0,16).replace("T"," ")+" · "+commitRow.detail.id.slice(0,10)):""
                            }
                            Repeater {
                                model:commitRow.detail?.files ?? []
                                ItemDelegate {
                                    required property var modelData
                                    Layout.fillWidth:true;implicitHeight:20;padding:0
                                    readonly property var info:panel.status(modelData.status)
                                    onClicked:panel.inspect("show_commit_file",commitRow.detail.id,modelData.path)
                                    contentItem:RowLayout {
                                        spacing:5
                                        Image { source:panel.controller.file_icon(parent.parent.modelData.path);Layout.preferredWidth:13;Layout.preferredHeight:13 }
                                        Label { text:panel.controller.path_name(parent.parent.modelData.path);font.pixelSize:11;font.strikeout:parent.parent.info[0]==="D" }
                                        Label { Layout.fillWidth:true;text:panel.controller.path_parent(parent.parent.modelData.path);opacity:0.55;font.pixelSize:10;elide:Text.ElideLeft }
                                        Label { text:parent.parent.info[0];font.family:"monospace";font.bold:true;color:panel.tone(parent.parent.info[1]) }
                                    }
                                }
                            }
                            Button { visible:!!commitRow.detail;flat:true;text:"Full Diff";onClicked:panel.inspect("show_commit",commitRow.detail.id) }
                        }
                    }
                }
                Menu {
                    id:commitMenu
                    MenuItem { text:"Show Changes";onTriggered:panel.inspect("show_commit",commitRow.modelData.commit.id) }
                    MenuItem { text:"Checkout (Detached)";onTriggered:panel.act("checkout",commitRow.modelData.commit.id) }
                    MenuItem { text:"Create Branch Here…";onTriggered:panel.openAction(panel.spec("branch"),commitRow.modelData.commit.id) }
                    MenuItem { text:"Create Tag Here…";onTriggered:panel.openAction(panel.spec("tag"),commitRow.modelData.commit.id) }
                    MenuSeparator {}
                    MenuItem { text:"Merge into Current Branch";onTriggered:panel.act("merge",commitRow.modelData.commit.id) }
                    MenuItem { text:"Rebase Current Branch onto This…";onTriggered:panel.confirm("rebase",commitRow.modelData.commit.id) }
                    MenuItem { text:"Cherry-pick";onTriggered:panel.act("cherry_pick",commitRow.modelData.commit.id) }
                    MenuItem { text:"Revert Commit";onTriggered:panel.act("revert",commitRow.modelData.commit.id) }
                    MenuItem { text:"Compare with HEAD";onTriggered:panel.inspect("compare",commitRow.modelData.commit.id,"HEAD") }
                }
            }
            footer:Button { text:"Load More Commits";flat:true;visible:history.count>=(panel.gitState.history_limit ?? 200);enabled:!panel.busy;onClicked:panel.controller.git_load_more() }
            ScrollBar.vertical:ScrollBar {}
        }
    }
    Menu {
        id:refMenu
        property var target:({name:"",kind:"",current:false})
        MenuItem { text:"Checkout "+refMenu.target.name;visible:refMenu.target.kind==="branch"&&!refMenu.target.current;height:visible?implicitHeight:0;onTriggered:panel.act("checkout",refMenu.target.name) }
        MenuItem { text:"Merge into Current Branch";visible:(refMenu.target.kind==="branch"||refMenu.target.kind==="remote")&&!refMenu.target.current;height:visible?implicitHeight:0;onTriggered:panel.act("merge",refMenu.target.name) }
        MenuItem { text:"Rebase Current Branch onto This…";visible:(refMenu.target.kind==="branch"||refMenu.target.kind==="remote")&&!refMenu.target.current;height:visible?implicitHeight:0;onTriggered:panel.confirm("rebase",refMenu.target.name) }
        MenuItem { text:"Rename…";visible:refMenu.target.kind==="branch";height:visible?implicitHeight:0;onTriggered:panel.openAction(panel.spec("rename_branch"),refMenu.target.name) }
        MenuItem { text:"Delete…";visible:refMenu.target.kind==="branch"&&!refMenu.target.current;height:visible?implicitHeight:0;onTriggered:panel.openAction(panel.spec("delete_branch"),refMenu.target.name) }
        MenuItem { text:"Delete Tag…";visible:refMenu.target.kind==="tag";height:visible?implicitHeight:0;onTriggered:panel.confirm("tag_delete",refMenu.target.name) }
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
        readonly property bool comparable:!!panel.gitState.diff && panel.gitState.diff.before !== null && panel.gitState.diff.after !== null
        readonly property bool conflict:!!panel.gitState.inspection?.conflict
        readonly property bool unified:!conflict && !(sideBySide.checked && comparable) && (panel.gitState.patch_lines ?? []).length>0
        ColumnLayout {
            anchors.fill:parent
            RowLayout {
                Label { Layout.fillWidth:true;opacity:0.7;text:panel.gitState.diff?(panel.gitState.diff.staged?"Staged changes (index ↔ HEAD)":"Working tree changes (file ↔ index)"):"" }
                CheckBox { id:sideBySide;objectName:"gitDiffMode";text:"Side by Side";checked:true;visible:diffDialog.comparable }
                Button {
                    visible:!!panel.gitState.diff&&!(sideBySide.checked&&diffDialog.comparable);enabled:panel.selectedLines.length>0&&!panel.busy;highlighted:true
                    text:(panel.gitState.diff?.staged?"Unstage ":"Stage ")+panel.selectedLines.length+" Lines"
                    onClicked:{panel.controller.git_stage_lines(JSON.stringify(panel.selectedLines));diffDialog.close()}
                }
            }
            SplitView {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:sideBySide.checked && diffDialog.comparable && !diffDialog.conflict && !panel.busy
                ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;selectByMouse:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.diff?.before ?? "" } }
                ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;selectByMouse:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.diff?.after ?? "" } }
            }
            ColumnLayout {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:diffDialog.conflict
                SplitView {
                    Layout.fillWidth:true;Layout.preferredHeight:180
                    ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.inspection?.conflict?.current ?? "" } }
                    ScrollView { SplitView.fillWidth:true;TextArea { readOnly:true;font.family:"monospace";textFormat:TextEdit.PlainText;text:panel.gitState.inspection?.conflict?.incoming ?? "" } }
                }
                RowLayout {
                    Button { text:"Accept Current";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_current }
                    Button { text:"Accept Incoming";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_incoming }
                    Button { text:"Accept Both";onClicked:mergeDraft.text=panel.gitState.inspection.conflict.accept_both }
                    Item { Layout.fillWidth:true }
                    Button { text:"Save and Stage Result";highlighted:true;enabled:!panel.busy;onClicked:{panel.controller.git_resolve(mergeDraft.text);diffDialog.close()} }
                }
                ScrollView { Layout.fillWidth:true;Layout.fillHeight:true;TextArea { id:mergeDraft;font.family:"monospace";textFormat:TextEdit.PlainText;selectByMouse:true } }
            }
            ListView {
                objectName:"gitPatchLines";Layout.fillWidth:true;Layout.fillHeight:true;clip:true
                visible:diffDialog.unified
                model:panel.gitState.patch_lines ?? []
                flickableDirection:Flickable.HorizontalAndVerticalFlick;contentWidth:Math.max(width,2000)
                delegate:ItemDelegate {
                    id:patchRow
                    required property var modelData
                    readonly property bool staging:!!panel.gitState.diff
                    readonly property bool chosen:panel.selectedLines.includes(modelData.index)
                    text:modelData.text.replace(/\n$/, "")
                    width:ListView.view.contentWidth;height:18;padding:0
                    onClicked:{
                        if(!staging) return
                        const lines=panel.gitState.patch_lines
                        let targets=[modelData.index]
                        if(modelData.kind==="hunk"){ targets=[]; for(let i=lines.findIndex(l=>l.index===modelData.index)+1;i<lines.length&&lines[i].kind!=="hunk"&&lines[i].kind!=="meta";i++) if(lines[i].changed) targets.push(lines[i].index) }
                        else if(!modelData.changed) return
                        const all=targets.every(t=>panel.selectedLines.includes(t))
                        panel.selectedLines=all?panel.selectedLines.filter(i=>!targets.includes(i)):panel.selectedLines.concat(targets.filter(t=>!panel.selectedLines.includes(t)))
                    }
                    background:Rectangle {
                        color:patchRow.chosen?Qt.alpha(Kirigami.Theme.highlightColor,0.35):patchRow.modelData.kind==="added"?Qt.alpha("#2ea043",0.18):patchRow.modelData.kind==="removed"?Qt.alpha("#f85149",0.18):patchRow.modelData.kind==="hunk"?Qt.alpha("#388bfd",0.12):"transparent"
                    }
                    contentItem:Row {
                        spacing:0
                        Label { width:18;text:patchRow.staging&&patchRow.modelData.changed?(patchRow.chosen?"☑":"☐"):"";font.pixelSize:12 }
                        Label { width:40;horizontalAlignment:Text.AlignRight;text:patchRow.modelData.old ?? "";font.family:"monospace";font.pixelSize:10;opacity:0.45 }
                        Label { width:40;horizontalAlignment:Text.AlignRight;text:patchRow.modelData.new ?? "";font.family:"monospace";font.pixelSize:10;opacity:0.45 }
                        Label {
                            leftPadding:8;text:patchRow.text;font.family:"monospace";textFormat:Text.PlainText
                            color:patchRow.modelData.kind==="added"?Kirigami.Theme.positiveTextColor:patchRow.modelData.kind==="removed"?Kirigami.Theme.negativeTextColor:patchRow.modelData.kind==="hunk"?Kirigami.Theme.linkColor:Kirigami.Theme.textColor
                            opacity:patchRow.modelData.kind==="meta"?0.6:1
                        }
                    }
                }
                ScrollBar.vertical:ScrollBar {}
                ScrollBar.horizontal:ScrollBar {}
            }
            ScrollView {
                Layout.fillWidth:true;Layout.fillHeight:true
                visible:!diffDialog.conflict && !diffDialog.unified && !(sideBySide.checked && diffDialog.comparable)
                TextArea {
                    objectName:"gitDiffText";readOnly:true;selectByMouse:true;wrapMode:TextEdit.NoWrap;textFormat:TextEdit.PlainText;font.family:"monospace"
                    text:panel.busy?"Loading…":panel.controller.error_message.length?panel.controller.error_message:panel.gitState.inspection?.text ?? panel.gitState.diff?.text ?? ""
                }
            }
        }
    }
}
