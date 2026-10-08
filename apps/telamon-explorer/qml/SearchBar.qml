pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The row under the toolbar while a search is open: where to look (This
// Folder, Everywhere), the filter chips (Kind, Modified, Size; each one that
// is set can be cleared with its x), and on the right how the search is going:
// the state of the index, or for a folder the index doesn't hold the live
// walk with its Stop button. It belongs to the tab shown (`search` is that
// tab's SearchController) and is open while there are words or chips, or the
// field in the toolbar or something in this row has the keyboard.
FocusScope {
    id: bar

    // The tab's SearchController, or null.
    property var search: null
    // The search field in the toolbar.
    property Item field: null

    readonly property bool menuOpen: kindMenu.visible || modifiedMenu.visible || sizeMenu.visible
    readonly property bool open: search !== null && (search.active || (field !== null && field.activeFocus) || bar.activeFocus || menuOpen)

    readonly property var kindNames: [qsTr("Any"), qsTr("Document"), qsTr("Image"), qsTr("Audio"), qsTr("Video"), qsTr("Archive"), qsTr("Code"), qsTr("Folder")]
    readonly property var modifiedNames: [qsTr("Any Time"), qsTr("Today"), qsTr("Past 7 Days"), qsTr("Past Month"), qsTr("Past Year")]
    readonly property var sizeNames: [qsTr("Any Size"), qsTr("Small"), qsTr("Medium"), qsTr("Large")]

    // Escape in this row: end the search.
    signal closeRequested

    Keys.onEscapePressed: bar.closeRequested()

    visible: open
    implicitHeight: open ? row.implicitHeight + Kirigami.Units.smallSpacing * 2 : 0
    Accessible.role: Accessible.ToolBar
    Accessible.name: qsTr("Search Options")

    // One filter's menu: "Any" first, then the choices, one chosen.
    component FilterMenu: ContextMenu {
        id: menu
        property var names: []
        property int current: 0
        // Words after the name, e.g. "Small (under 1 MB)".
        property var hints: []
        signal chosen(int value)
        Repeater {
            model: menu.names
            ContextMenuItem {
                required property string modelData
                required property int index
                text: menu.hints[index] ? menu.hints[index] : modelData
                radio: true
                checkable: true
                checked: menu.current === index
                onTriggered: menu.chosen(index)
            }
        }
    }

    FilterMenu {
        id: kindMenu
        names: bar.kindNames
        current: bar.search ? bar.search.kind : 0
        onChosen: value => bar.search.kind = value
    }
    FilterMenu {
        id: modifiedMenu
        names: bar.modifiedNames
        current: bar.search ? bar.search.modified : 0
        onChosen: value => bar.search.modified = value
    }
    FilterMenu {
        id: sizeMenu
        names: bar.sizeNames
        current: bar.search ? bar.search.size : 0
        hints: bar.search ? [bar.sizeNames[0], bar.search.sizeHint(1), bar.search.sizeHint(2), bar.search.sizeHint(3)] : []
        onChosen: value => bar.search.size = value
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing * 2

        Row {
            id: scopeChips
            spacing: Kirigami.Units.smallSpacing
            Accessible.role: Accessible.Grouping
            Accessible.name: qsTr("Where to Search")
            TelamonChip {
                text: qsTr("This Folder")
                checkable: true
                autoExclusive: true
                checked: bar.search ? bar.search.scope === 0 : true
                onClicked: bar.search.scope = 0
            }
            TelamonChip {
                text: qsTr("Everywhere")
                checkable: true
                autoExclusive: true
                checked: bar.search ? bar.search.scope === 1 : false
                onClicked: bar.search.scope = 1
            }
        }

        Rectangle {
            implicitWidth: 1
            implicitHeight: Kirigami.Units.gridUnit
            color: Qt.alpha(Kirigami.Theme.textColor, 0.2)
        }

        Row {
            id: filterChips
            spacing: Kirigami.Units.smallSpacing
            Accessible.role: Accessible.Grouping
            Accessible.name: qsTr("Filters")
            // Set by a tag in the sidebar: only items with this tag.
            TelamonChip {
                id: tagChip
                visible: bar.search !== null && bar.search.tag.length > 0
                text: bar.search ? qsTr("Tag: %1").arg(TagLogic.shown(bar.search.tag)) : ""
                closable: true
                onCloseRequested: bar.search.tag = ""
            }
            TelamonChip {
                id: kindChip
                text: bar.search && bar.search.kind !== 0 ? qsTr("Kind: %1").arg(bar.kindNames[bar.search.kind]) : qsTr("Kind")
                closable: bar.search !== null && bar.search.kind !== 0
                onClicked: kindMenu.popup(kindChip, 0, kindChip.height + Kirigami.Units.smallSpacing)
                onCloseRequested: bar.search.kind = 0
            }
            TelamonChip {
                id: modifiedChip
                text: bar.search && bar.search.modified !== 0 ? qsTr("Modified: %1").arg(bar.modifiedNames[bar.search.modified]) : qsTr("Modified")
                closable: bar.search !== null && bar.search.modified !== 0
                onClicked: modifiedMenu.popup(modifiedChip, 0, modifiedChip.height + Kirigami.Units.smallSpacing)
                onCloseRequested: bar.search.modified = 0
            }
            TelamonChip {
                id: sizeChip
                text: bar.search && bar.search.size !== 0 ? qsTr("Size: %1").arg(bar.sizeNames[bar.search.size]) : qsTr("Size")
                closable: bar.search !== null && bar.search.size !== 0
                onClicked: sizeMenu.popup(sizeChip, 0, sizeChip.height + Kirigami.Units.smallSpacing)
                onCloseRequested: bar.search.size = 0
            }
        }

        Item {
            Layout.fillWidth: true
        }

        // A live search: the walk's progress, and Stop.
        TelamonSpinner {
            visible: bar.search !== null && bar.search.walking
            running: visible
            Layout.preferredWidth: Kirigami.Units.iconSizes.small
            Layout.preferredHeight: Kirigami.Units.iconSizes.small
        }
        Text {
            visible: bar.search !== null && bar.search.live
            textFormat: Text.PlainText
            text: bar.search ? bar.search.statusText : ""
            elide: Text.ElideRight
            Layout.maximumWidth: Kirigami.Units.gridUnit * 20
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
            Accessible.role: Accessible.StaticText
            Accessible.name: text
        }
        SecondaryButton {
            id: stopButton
            visible: bar.search !== null && bar.search.walking
            text: qsTr("Stop")
            symbol: Symbols.Stop
            onClicked: bar.search.stop()
        }

        // The state of the index, for a search that asks it.
        Rectangle {
            id: chip
            visible: bar.search !== null && !bar.search.live && bar.search.chipText.length > 0
            readonly property int level: bar.search ? bar.search.chipLevel : 0
            readonly property color tint: level === 3 ? Kirigami.Theme.negativeTextColor : (level === 2 ? Kirigami.Theme.neutralTextColor : (level === 1 ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.textColor))
            Layout.maximumWidth: Kirigami.Units.gridUnit * 30
            implicitWidth: chipRow.implicitWidth + Kirigami.Units.gridUnit
            implicitHeight: Math.round(Kirigami.Units.gridUnit * 1.5)
            radius: height / 2
            color: Qt.alpha(tint, level === 0 ? 0.06 : 0.12)
            border.width: 1
            border.color: Qt.alpha(tint, 0.35)
            Accessible.role: Accessible.StaticText
            Accessible.name: bar.search ? bar.search.chipText : ""
            Row {
                id: chipRow
                anchors.centerIn: parent
                width: Math.min(implicitWidth, chip.width - Kirigami.Units.gridUnit)
                spacing: Kirigami.Units.smallSpacing
                Symbol {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: chip.level === 3 ? Symbols.Error : (chip.level === 2 ? Symbols.Sync : (chip.level === 1 ? Symbols.CheckCircle : Symbols.Info))
                    size: Math.round(Kirigami.Units.iconSizes.small * 0.9)
                    color: chip.tint
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: bar.search ? bar.search.chipText : ""
                    elide: Text.ElideRight
                    width: Math.min(implicitWidth, chip.Layout.maximumWidth - Kirigami.Units.gridUnit * 3)
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: Kirigami.Theme.textColor
                    Accessible.ignored: true
                }
            }
        }
    }
}
