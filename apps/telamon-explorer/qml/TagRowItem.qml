pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A row of the Tags menu that holds the seven colour tags as dots side by
// side: one row of the menu, so the arrow keys land on it, Left and Right pick
// a dot and Enter or Space presses it. A dot with a check mark is on every
// selected item, one with a dash on some of them. Telamon.Ui has no such row
// yet; this has the shape it would ask for, `colours` being [{name, colour,
// state}] (state 0 none, 1 some, 2 all) and `chosen(name, on)`: on when the
// tag is to be put on the items that lack it.
T.MenuItem {
    id: control

    property var colours: []
    // The dot the keyboard is on (-1: none yet).
    property int current: -1
    signal chosen(string name, bool on)

    readonly property real dotCell: Math.round(Kirigami.Units.gridUnit * 2.0)
    implicitWidth: Math.max(colours.length * dotCell, Kirigami.Units.gridUnit * 11 - 2 * TelamonStyle.spacingSmall)
    // A hidden row takes no room (the items can't keep tags: the menu says why).
    implicitHeight: visible ? Math.round(Kirigami.Units.gridUnit * 2.4) : 0
    padding: 0
    hoverEnabled: true

    Accessible.role: Accessible.Grouping
    Accessible.name: qsTr("Colour Tags")

    onHighlightedChanged: current = highlighted && colours.length > 0 ? 0 : -1

    function _press(i) {
        if (i >= 0 && i < colours.length) {
            chosen(colours[i].name, colours[i].state !== 2);
        }
    }

    Keys.onPressed: event => {
        // Up, Down and the rest are the menu's.
        event.accepted = false;
        if (event.key === Qt.Key_Left || event.key === Qt.Key_Right) {
            const dir = (event.key === Qt.Key_Right) !== control.mirrored ? 1 : -1;
            const next = current + dir;
            if (next >= 0 && next < colours.length) {
                current = next;
            }
            event.accepted = true;
        } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
            _press(current);
            event.accepted = true;
        }
    }

    background: Item {}

    contentItem: Row {
        spacing: 0
        Repeater {
            model: control.colours
            delegate: Item {
                id: cell
                required property var modelData
                required property int index
                width: control.dotCell
                height: control.height
                readonly property bool active: area.containsMouse || (control.highlighted && control.current === index)

                Rectangle {
                    anchors.centerIn: parent
                    width: Math.round(Kirigami.Units.gridUnit * 1.5)
                    height: width
                    radius: width / 2
                    color: cell.modelData.colour
                    border.width: cell.active ? 2 : 0
                    border.color: TelamonStyle.text
                }
                Symbol {
                    anchors.centerIn: parent
                    visible: cell.modelData.state !== 0
                    icon: cell.modelData.state === 2 ? Symbols.Check : Symbols.Remove
                    size: Math.round(Kirigami.Units.iconSizes.small)
                    color: "white" // telamon-lint: allow-raw (a mark on a colour tag's dot)
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    onEntered: control.current = cell.index
                    onClicked: control._press(cell.index)
                }
                TelamonToolTip {
                    text: cell.modelData.name
                    shown: area.containsMouse
                }
                Accessible.role: Accessible.CheckBox
                Accessible.name: cell.modelData.name
                Accessible.checkable: true
                Accessible.checked: cell.modelData.state === 2
                Accessible.onPressAction: control._press(cell.index)
            }
        }
    }
}
