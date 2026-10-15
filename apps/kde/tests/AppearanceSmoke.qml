import QtQuick
import QtQuick.Controls
import QtQml.Models
import org.kde.kirigami as Kirigami
import org.nativeworkspace.Diagnostic
import "../qml"

Kirigami.ApplicationWindow {
    id: window
    width: 900
    height: 600
    visible: true
    AppController { id: controller }
    Pane {
        id: preview
        anchors.fill: parent
        padding: 0
        WorkspacePane { id: pane; anchors.fill: parent; controller: controller }
    }
    property int step: 0
    property bool capturing: false
    property var tree
    SystemPalette { id: systemColors; colorGroup: SystemPalette.Active }
    Window {
        id: focusWindow
        transientParent: null
        flags: Qt.Window
        width: 200
        height: 100
        title: "Selection focus check"
    }
    property int focusAttempts: 0

    function check(condition, message) { if (!condition) throw new Error(message) }
    function findItem(item, name) {
        if (item.objectName === name) return item
        for (var child of item.children || []) {
            var found = findItem(child, name)
            if (found) return found
        }
        return null
    }
    function luminance(color) {
        function linear(value) { return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4) }
        return 0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }
    function checkSystemTheme(dark) {
        check((luminance(pane.Kirigami.Theme.backgroundColor) < 0.5) === dark,
            "Workspace did not follow system appearance: " + pane.Kirigami.Theme.backgroundColor)
        check((luminance(pane.homeWindow.Kirigami.Theme.backgroundColor) < 0.5) === dark,
            "Home did not follow system appearance")
        check((luminance(findItem(pane, "documentEditor").Kirigami.Theme.backgroundColor) < 0.5) === dark,
            "Editor did not follow system appearance")
    }
    function checkAppearance(dark) {
        tree.forceLayout()
        check(Qt.styleHints.colorScheme === (dark ? Qt.Dark : Qt.Light), "Qt did not change the application appearance")
        checkSystemTheme(dark)
        var activeLine = pane.editorActiveLine
        var lineColor = activeLine.color
        check(activeLine.visible, "Editor did not show the cursor line")
        check(Math.abs(lineColor.r - systemColors.accent.r) < 0.005
            && Math.abs(lineColor.g - systemColors.accent.g) < 0.005
            && Math.abs(lineColor.b - systemColors.accent.b) < 0.005,
            "Cursor line did not use the system accent: " + lineColor + " vs " + systemColors.accent)
        check(lineColor.a > 0 && lineColor.a < 0.25,
            "Cursor line accent must stay subtle enough to read syntax colors")
        for (var path of ["src", "src/main.rs"]) {
            var row = tree.itemAtCell(Qt.point(0, pane.treeRowForPath(path)))
            check(row && row.contentItem.title === path.split("/").pop(), "Missing file row: " + path
                + " index=" + pane.treeRowForPath(path) + " title=" + (row ? row.contentItem.title : "no delegate")
                + " rows=" + tree.rows + " expanded=" + pane.collapsedDirectories.src)
            var icon = row.contentItem.children.find(child => child.source !== undefined && child.valid !== undefined)
            check(icon && icon.valid && icon.width > 0, "Material icon did not load: " + path)
            var iconName = path === "src" ? "folder-src-open" : "rust"
            check(icon.source.toString().endsWith("/material/" + iconName + ".png"),
                "File row did not load the shared Material icon: " + path)
        }
        var selected = tree.itemAtCell(Qt.point(0, pane.treeRowForPath("src/main.rs")))
        check(selected.highlighted && selected.contentItem.selected, "File row did not show selection")
        check(selected.background.color.toString() === systemColors.accent.toString(),
            "File selection did not use the system accent: " + selected.background.color + " vs " + systemColors.accent)
        check(selected.palette.inactive.highlight.toString() === systemColors.accent.toString(),
            "Inactive file selection did not use the system accent")
        check(selected.background.color.toString() !== selected.palette.base.toString(),
            "Selected file row has the same background as an unselected row")
        check(selected.contentItem.color.toString() === systemColors.highlightedText.toString(),
            "File row did not use the theme selection text")
        var foreground = luminance(selected.contentItem.color)
        var background = luminance(selected.background.color)
        var contrast = (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05)
        check(contrast > 2, "File selection is unreadable: contrast=" + contrast)
        console.log("KDE appearance=" + (dark ? "dark" : "light") + " active=" + window.active
            + " selection=" + selected.background.color + " contrast=" + contrast)
        var capture = Qt.application.arguments.find(argument => argument.startsWith("--smoke-screenshot="))
        if (capture) {
            capturing = true
            check(preview.grabToImage(function(result) {
                if (!result.saveToFile(capture.substring("--smoke-screenshot=".length) + (dark ? "-dark.png" : "-light.png"))) Qt.exit(1)
                capturing = false
            }), "Could not capture appearance")
        }
    }
    Component.onCompleted: {
        var argument = Qt.application.arguments.find(value => value.startsWith("--smoke-appearance="))
        controller.open_workspace(argument.substring("--smoke-appearance=".length))
        pane.showingHome = false
        tree = findItem(pane, "fileTree")
    }
    Timer {
        interval: 250
        running: true
        repeat: true
        onTriggered: {
            if (capturing) return
            try {
                switch (step++) {
                case 0:
                    checkSystemTheme(Qt.styleHints.colorScheme === Qt.Dark)
                    pane.toggleDirectory("src")
                    controller.open_document("src/main.rs")
                    Qt.styleHints.colorScheme = Qt.Light
                    break
                case 1:
                    checkAppearance(false)
                    findItem(pane, "documentEditor").forceActiveFocus()
                    break
                case 2:
                    check(findItem(pane, "documentEditor").activeFocus, "Editor did not receive focus")
                    checkAppearance(false)
                    focusWindow.show()
                    focusWindow.requestActivate()
                    break
                case 3:
                    if (window.active && ++focusAttempts < 8) {
                        focusWindow.raise()
                        focusWindow.requestActivate()
                        --step
                        return
                    }
                    check(!window.active, "Focus test did not deactivate the workspace")
                    checkAppearance(false)
                    focusWindow.hide()
                    window.requestActivate()
                    Qt.styleHints.colorScheme = Qt.Dark
                    break
                case 4:
                    checkAppearance(true)
                    break
                case 5:
                    focusAttempts = 0
                    focusWindow.show()
                    focusWindow.requestActivate()
                    break
                case 6:
                    if (window.active && ++focusAttempts < 8) {
                        focusWindow.raise()
                        focusWindow.requestActivate()
                        --step
                        return
                    }
                    check(!window.active, "Focus test did not deactivate the dark workspace")
                    checkAppearance(true)
                    focusWindow.hide()
                    window.requestActivate()
                    Qt.styleHints.colorScheme = Qt.Light
                    break
                case 7:
                    checkAppearance(false)
                    break
                case 8:
                    Qt.styleHints.colorScheme = Qt.Dark
                    break
                case 9:
                    checkAppearance(true)
                    break
                case 10:
                    Qt.styleHints.colorScheme = Qt.Unknown
                    console.log("KDE system appearance/icons/selection smoke test passed")
                    Qt.quit()
                }
            } catch (error) { console.error(error); Qt.exit(1) }
        }
    }
}
