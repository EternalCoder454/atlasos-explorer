pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The folder filter of one pane (Ctrl+F): a field that narrows the items of
// the folder as you type, and, while it is narrowing, a chip that cannot be
// missed - "Showing 4 of 120 · Clear" - with the row tinted, so a filter is
// never forgotten. Escape (or Clear, or the x) ends it. The filter belongs
// to the pane's FolderModel; a search (Ctrl+E) or another folder ends it.
// Ctrl+F while results are shown goes to the search field instead.
Rectangle {
    id: bar

    // The pane's FolderModel and SearchController.
    required property var folder
    required property var search
    // The row is shown (Ctrl+F opened it).
    property bool open: false
    readonly property alias field: field
    readonly property bool editing: field.activeFocus || filtersButton.activeFocus || clearButton.activeFocus || filtersPopover.visible

    // Escape or the x: the filter is gone and the keyboard goes back to the items.
    signal dismissed
    // Down or Enter in the field: the keyboard goes to the items, the filter stays.
    signal listRequested

    visible: open && !folder.searching
    implicitHeight: visible ? row.implicitHeight + Kirigami.Units.smallSpacing * 2 : 0
    height: implicitHeight
    color: folder.filterActive ? Qt.alpha(TelamonStyle.accent, 0.12) : Qt.alpha(Kirigami.Theme.textColor, 0.04)
    Accessible.role: Accessible.ToolBar
    Accessible.name: qsTr("Filter This Folder")

    // Opens the row and puts the keyboard in the field.
    function show() {
        open = true;
        field.forceActiveFocus(Qt.ShortcutFocusReason);
        field.selectAll();
    }
    // Ends the filter: the text goes and the row closes.
    function dismiss() {
        folder.clearFilter();
        open = false;
        dismissed();
    }

    // A search or another folder takes the filter away: the row goes too.
    Connections {
        target: bar.folder
        function onSearchingChanged() {
            if (bar.folder.searching) {
                bar.open = false;
            }
        }
        // (A redirect also changes the url: a filter that is on keeps its row.)
        function onUrlChanged() {
            if (!bar.folder.filterActive && bar.folder.filterText.length === 0) {
                bar.open = false;
            }
        }
    }

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing * 2
        anchors.rightMargin: Kirigami.Units.smallSpacing * 2
        spacing: Kirigami.Units.smallSpacing * 2

        SearchField {
            id: field
            Layout.preferredWidth: Kirigami.Units.gridUnit * 16
            placeholderText: qsTr("Filter This Folder")
            Accessible.name: qsTr("Filter This Folder")
            text: bar.folder.filterText
            onTextChanged: {
                if (bar.folder.filterText !== text) {
                    bar.folder.filterText = text;
                }
            }
            // An empty row that lost the keyboard goes away.
            onActiveFocusChanged: {
                if (!activeFocus && !bar.folder.filterActive && bar.folder.filterError.length === 0 && !filtersPopover.visible && text.length === 0) {
                    bar.open = false;
                }
            }
            // The field's own Escape clears its text first (and stops there); this one
            // runs after it and ends the filter, so a single Escape does both.
            Keys.onEscapePressed: event => {
                bar.dismiss();
                event.accepted = true;
            }
            // Down and Enter go to the items (the field lets Return and Enter through).
            Keys.onPressed: event => {
                if (event.key === Qt.Key_Down || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                    bar.listRequested();
                    event.accepted = true;
                }
            }
        }

        // Showing 4 of 120 · Clear
        Rectangle {
            id: chip
            visible: bar.folder.filterActive
            implicitWidth: chipRow.implicitWidth + Kirigami.Units.gridUnit
            implicitHeight: Math.round(Kirigami.Units.gridUnit * 1.6)
            radius: height / 2
            color: Qt.alpha(TelamonStyle.accent, 0.2)
            border.width: 1
            border.color: Qt.alpha(TelamonStyle.accent, 0.6)
            Accessible.role: Accessible.StaticText
            Accessible.name: qsTr("Showing %1 of %2").arg(bar.folder.count).arg(bar.folder.filterTotal)
            Row {
                id: chipRow
                anchors.centerIn: parent
                spacing: Kirigami.Units.smallSpacing
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: qsTr("Showing %1 of %2").arg(bar.folder.count).arg(bar.folder.filterTotal)
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    font.weight: Font.Medium
                    color: Kirigami.Theme.textColor
                    Accessible.ignored: true
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: "·"
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: TelamonStyle.textMuted
                    Accessible.ignored: true
                }
                TextButton {
                    id: clearButton
                    anchors.verticalCenter: parent.verticalCenter
                    height: chip.implicitHeight - 4
                    text: qsTr("Clear")
                    Accessible.name: qsTr("Clear Filter")
                    onClicked: bar.dismiss()
                }
            }
        }

        // The pattern is wrong: nothing is filtered, and it says why.
        Text {
            visible: bar.folder.filterError.length > 0
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 30
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: bar.folder.filterError
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            color: Kirigami.Theme.negativeTextColor
            Accessible.role: Accessible.StaticText
            Accessible.name: text
        }

        Item {
            Layout.fillWidth: true
        }

        SecondaryButton {
            id: filtersButton
            text: qsTr("Filters")
            symbol: Symbols.Tune
            Accessible.name: qsTr("Filters")
            onClicked: filtersPopover.open()
        }
        ToolbarButton {
            symbol: Symbols.Close
            text: qsTr("Close Filter")
            toolTipText: qsTr("End the filter (Esc)")
            focusable: true
            onClicked: bar.dismiss()
        }
    }

    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: bar.folder.filterActive ? Qt.alpha(TelamonStyle.accent, 0.5) : Qt.alpha(Kirigami.Theme.textColor, 0.12)
    }

    FiltersPopover {
        id: filtersPopover
        target: filtersButton
        search: bar.search
        error: bar.folder.filterError
    }
}
