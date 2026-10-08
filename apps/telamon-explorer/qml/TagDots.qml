import QtQuick
import Telamon.Ui

// The coloured dots of an item's colour tags (red, orange, ...), overlapping a
// little, at most four. `colours` are colours (`#rrggbb`), as the folder model's
// `tagColours`; nothing is drawn for an item with none. The tag names are in the
// Tags column, the context menu and Properties.
Row {
    id: dots

    property var colours: []
    // The width of one dot.
    property real dot: 10
    readonly property int shownCount: Math.min(4, colours ? colours.length : 0)

    visible: shownCount > 0
    spacing: -Math.round(dot * 0.3)
    // Decoration: the names are available in text elsewhere.
    Accessible.ignored: true

    Repeater {
        model: dots.shownCount
        Rectangle {
            id: one
            required property int index
            width: dots.dot
            height: dots.dot
            radius: width / 2
            color: dots.colours[one.index]
            border.width: 1
            border.color: Qt.alpha(TelamonStyle.base, 0.9)
        }
    }
}
