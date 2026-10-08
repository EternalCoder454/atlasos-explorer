pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Filters popover of the filter row and the search row: "Use pattern
// (regular expression)" for the name match, and the reason when the pattern
// is not a valid one (it is not run). The switch is the pane's
// (SearchController.usePattern): it drives the folder filter and the search
// of that pane. Patterns run in time linear in the text and are length- and
// size-capped, so a hostile one cannot hang the window (docs/DESIGN.md,
// "Search").
TelamonPopover {
    id: pop

    // The pane's SearchController.
    property var search: null
    // Why the words are not a valid pattern (empty: they are).
    property string error: ""

    readonly property real contentWidth_: Kirigami.Units.gridUnit * 22

    RowLayout {
        Layout.preferredWidth: pop.contentWidth_
        spacing: TelamonStyle.spacingLarge
        TelamonSwitch {
            id: toggle
            checked: pop.search ? pop.search.usePattern : false
            Accessible.name: qsTr("Use pattern (regular expression)")
            onToggled: {
                if (pop.search) {
                    pop.search.usePattern = checked;
                }
            }
        }
        Text {
            Layout.fillWidth: true
            text: qsTr("Use pattern (regular expression)")
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: Kirigami.Theme.textColor
            Accessible.ignored: true
            TapHandler {
                onTapped: toggle.toggle()
            }
        }
    }
    Text {
        Layout.preferredWidth: pop.contentWidth_
        text: qsTr("The pattern is matched against each file name, ignoring case. For example, ^IMG_[0-9]+ finds names that start with IMG_ and a number. With Inside Files it is matched against each line.")
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: TelamonStyle.textMuted
    }
    Text {
        id: errorLine
        visible: pop.error.length > 0
        Layout.preferredWidth: pop.contentWidth_
        text: pop.error
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.negativeTextColor
        Accessible.role: Accessible.StaticText
        Accessible.name: text
    }
}
