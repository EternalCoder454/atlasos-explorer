pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A row of the context menu that holds a few icon buttons side by side (Cut,
// Copy, Paste, Rename, Trash): one row of the menu, so the arrow keys land on
// it, Left and Right pick a button and Enter or Space presses it. Telamon.Ui
// has no such row yet; this has the shape it would ask for, `buttons` being
// [{symbol, text, enabled, destructive, command}] and `chosen(command)`.
T.MenuItem {
    id: control

    property var buttons: []
    // The button the keyboard is on (-1: none yet).
    property int current: -1
    // A button was pressed; the menu closes after it.
    signal chosen(string command)

    implicitWidth: Math.max(buttons.length * button, Kirigami.Units.gridUnit * 11 - 2 * TelamonStyle.spacingSmall)
    implicitHeight: Math.round(Kirigami.Units.gridUnit * 2.4)
    readonly property real button: Math.round(Kirigami.Units.gridUnit * 2.4)
    padding: 0
    hoverEnabled: true

    Accessible.role: Accessible.Grouping
    Accessible.name: qsTr("Quick Actions")

    // The row got the highlight (arrow keys): start on the first usable button.
    onHighlightedChanged: {
        if (highlighted) {
            current = _step(-1, 1);
        } else {
            current = -1;
        }
    }

    function _usable(i) {
        return i >= 0 && i < buttons.length && buttons[i].enabled !== false;
    }
    // The next usable button from `from` in `dir` (-1 when none).
    function _step(from, dir) {
        for (let i = from + dir; i >= 0 && i < buttons.length; i += dir) {
            if (_usable(i)) {
                return i;
            }
        }
        return -1;
    }
    function _press(i) {
        if (_usable(i)) {
            chosen(buttons[i].command);
        }
    }

    Keys.onPressed: event => {
        // Up, Down and the rest are the menu's: a handler accepts every key it
        // sees unless it says otherwise.
        event.accepted = false;
        const rtl = control.mirrored;
        if (event.key === Qt.Key_Left || event.key === Qt.Key_Right) {
            const dir = (event.key === Qt.Key_Right) !== rtl ? 1 : -1;
            const next = _step(current, dir);
            if (next >= 0) {
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
            model: control.buttons
            delegate: Item {
                id: cell
                required property var modelData
                required property int index
                readonly property bool usable: modelData.enabled !== false
                width: control.button
                height: control.height
                readonly property bool active: (area.containsMouse && usable) || (control.highlighted && control.current === index)

                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 2
                    radius: TelamonStyle.radiusSmall
                    color: cell.active ? (cell.modelData.destructive === true ? TelamonStyle.errorFill : (area.pressed ? TelamonStyle.pressed : TelamonStyle.hover)) : "transparent"
                }
                Symbol {
                    anchors.centerIn: parent
                    icon: cell.modelData.symbol
                    size: Math.round(Kirigami.Units.iconSizes.small * 1.25)
                    color: !cell.usable ? TelamonStyle.textDisabled : cell.modelData.destructive === true ? TelamonStyle.error : TelamonStyle.text
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    onEntered: if (cell.usable) {
                        control.current = cell.index
                    }
                    onClicked: control._press(cell.index)
                }
                TelamonToolTip {
                    text: cell.modelData.text
                    shown: area.containsMouse
                }
                Accessible.role: Accessible.Button
                Accessible.name: cell.modelData.text
                Accessible.onPressAction: control._press(cell.index)
            }
        }
    }
}
