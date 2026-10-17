import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: panel
    required property var workspace
    readonly property var controller: workspace.controller
    property bool showReplace: false
    property bool showDetails: false
    spacing: Kirigami.Units.smallSpacing
    function focusReplace() {
        showReplace = true
        Qt.callLater(function() { replacement.forceActiveFocus() })
    }
    function configure() {
        controller.configure_search_files(includeField.text, excludeField.text, ignoreFiles.checked, openEditors.checked)
    }
    function replace(path, index) {
        if (workspace.flushEditor()) controller.prepare_replacement(replacement.text, preserveCase.checked, path, index)
    }
    RowLayout {
        Layout.fillWidth: true
        ToolButton {
            text: "Aa"; checkable: true; checked: panel.controller.search_match_case
            Accessible.name: "Match Case"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.configure_search(checked, panel.controller.search_whole_word, panel.controller.search_regex)
        }
        ToolButton {
            text: "ab"; checkable: true; checked: panel.controller.search_whole_word
            Accessible.name: "Match Whole Word"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.configure_search(panel.controller.search_match_case, checked, panel.controller.search_regex)
        }
        ToolButton {
            text: ".*"; checkable: true; checked: panel.controller.search_regex
            Accessible.name: "Use Regular Expression"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.configure_search(panel.controller.search_match_case, panel.controller.search_whole_word, checked)
        }
        ToolButton {
            icon.name: "view-refresh"; Accessible.name: "Refresh Search"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.refresh_search()
        }
        ToolButton {
            icon.name: "process-stop"; visible: panel.controller.search_in_progress
            Accessible.name: "Cancel Search"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.cancel_search()
        }
        ToolButton {
            icon.name: "edit-clear"; Accessible.name: "Clear Search"
            ToolTip.visible: hovered; ToolTip.text: Accessible.name
            onClicked: panel.controller.search_files("")
        }
    }
    ToolButton {
        text: "Replace"; icon.name: panel.showReplace ? "arrow-down" : "arrow-right"
        checkable: true; checked: panel.showReplace
        onClicked: panel.showReplace = checked
    }
    ColumnLayout {
        visible: panel.showReplace
        Layout.fillWidth: true
        RowLayout {
            Layout.fillWidth: true
            TextField { id: replacement; objectName: "searchReplacement"; Layout.fillWidth: true; placeholderText: "Replace" }
            ToolButton {
                id: preserveCase; text: "AB"; checkable: true; Accessible.name: "Preserve Case"
                ToolTip.visible: hovered; ToolTip.text: Accessible.name
            }
        }
        RowLayout {
            enabled: !panel.controller.move_pending && !panel.controller.search_in_progress
                && !panel.controller.search_truncated && panel.workspace.searchFileCount > 0
            Button {
                text: "Match…"; enabled: panel.workspace.selectedResultIndex >= 0
                onClicked: panel.replace("", panel.workspace.selectedResultIndex)
            }
            Button {
                text: "File…"; enabled: panel.controller.active_document_id.length > 0
                onClicked: panel.replace(panel.controller.active_document_id, -1)
            }
            Button { text: "All…"; onClicked: panel.replace("", -1) }
        }
    }
    ToolButton {
        text: "Search details"; icon.name: panel.showDetails ? "arrow-down" : "arrow-right"
        checkable: true; checked: panel.showDetails
        onClicked: panel.showDetails = checked
    }
    ColumnLayout {
        visible: panel.showDetails
        Layout.fillWidth: true
        Label { text: "files to include"; font: Kirigami.Theme.smallFont }
        TextField {
            id: includeField; Layout.fillWidth: true; placeholderText: "e.g. *.rs, src/**"
            text: panel.controller.search_include
            Accessible.name: "Files to Include"
            onTextEdited: panel.configure()
        }
        CheckBox {
            id: openEditors; text: "Search only in Open Editors"; checked: panel.controller.search_open_editors
            onClicked: panel.configure()
        }
        Label { text: "files to exclude"; font: Kirigami.Theme.smallFont }
        TextField {
            id: excludeField; Layout.fillWidth: true; placeholderText: "e.g. target, **/*.lock"
            text: panel.controller.search_exclude
            Accessible.name: "Files to Exclude"
            onTextEdited: panel.configure()
        }
        CheckBox {
            id: ignoreFiles; text: "Use Ignore Files"; checked: panel.controller.search_ignore
            onClicked: panel.configure()
        }
    }
    Label {
        Layout.fillWidth: true
        text: panel.controller.search_error.length > 0 ? panel.controller.search_error
            : panel.controller.search_results.length + " results in " + panel.workspace.searchFileCount + " files"
        color: panel.controller.search_error.length > 0 ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
        font: Kirigami.Theme.smallFont
    }
    Connections {
        target: panel.controller
        function onReplacement_previewChanged() {
            if (panel.controller.replacement_preview.length > 0) preview.open()
            else preview.close()
        }
    }
    Kirigami.Dialog {
        id: preview
        objectName: "replacementPreview"
        parent: panel.workspace.Overlay.overlay
        readonly property var prompt: panel.controller.replacement_confirmation(panel.controller.replacement_occurrences, panel.controller.replacement_files)
        title: prompt.title
        preferredWidth: Kirigami.Units.gridUnit * 36
        preferredHeight: Kirigami.Units.gridUnit * 28
        modal: true
        closePolicy: Popup.NoAutoClose
        standardButtons: Kirigami.Dialog.NoButton
        ColumnLayout {
            Label { Layout.fillWidth: true; text: preview.prompt.body; wrapMode: Text.Wrap }
            ScrollView {
                Layout.fillWidth: true; Layout.fillHeight: true
                TextArea {
                    text: panel.controller.replacement_preview
                    readOnly: true; selectByMouse: true; font.family: "monospace"
                }
            }
            Label { Layout.fillWidth: true; text: panel.controller.error_message; wrapMode: Text.Wrap; color: Kirigami.Theme.negativeTextColor }
        }
        customFooterActions: [
            Kirigami.Action { text: preview.prompt.cancel; onTriggered: panel.controller.discard_replacement() },
            Kirigami.Action {
                text: preview.prompt.confirm; enabled: !panel.controller.move_pending
                onTriggered: if (panel.workspace.flushEditor()) panel.controller.apply_replacement()
            }
        ]
    }
}
