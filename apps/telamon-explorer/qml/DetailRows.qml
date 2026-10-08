pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Names and values, one pair to a line: a muted name at the trailing edge of
// its column, the value beside it (selectable text, in the fixed-width font
// for a `mono` one, with a copy button for a `copyable` one). `model` is
// [{label, value, mono, copyable}], as TelamonDetailGrid's. Files has its own
// because Telamon.Ui's grid (2.0.0) loses its labels when its rows arrive
// after it was made, which is how Properties gets them (a framework gap, to
// be reported upstream; this has the same shape, so it can be swapped).
GridLayout {
    id: grid

    property var model: []
    readonly property int count: model !== null && typeof model === "object" && Number.isInteger(model.length) ? Math.min(model.length, 200) : 0

    Layout.fillWidth: true
    columns: 2
    columnSpacing: Kirigami.Units.largeSpacing
    rowSpacing: Kirigami.Units.smallSpacing + 2

    Repeater {
        model: grid.count * 2
        delegate: Item {
            id: cell
            required property int index
            readonly property int entryIndex: Math.floor(index / 2)
            readonly property bool isValue: index % 2 === 1
            readonly property var entry: grid.model[entryIndex] ?? ({})
            readonly property string text: String((isValue ? entry.value : entry.label) ?? "")

            Layout.fillWidth: isValue
            Layout.alignment: Qt.AlignTop
            Layout.minimumWidth: 0
            Layout.preferredWidth: isValue ? 1 : Kirigami.Units.gridUnit * 7
            implicitHeight: isValue ? Math.max(value.implicitHeight, copy.visible ? copy.implicitHeight : 0) : label.implicitHeight

            Text {
                id: label
                visible: !cell.isValue
                anchors.left: parent.left
                anchors.right: parent.right
                text: cell.text
                textFormat: Text.PlainText
                horizontalAlignment: Text.AlignRight
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
                Accessible.ignored: true
            }
            TextEdit {
                id: value
                visible: cell.isValue
                anchors.left: parent.left
                anchors.right: copy.visible ? copy.left : parent.right
                anchors.rightMargin: copy.visible ? Kirigami.Units.smallSpacing : 0
                text: cell.text
                readOnly: true
                selectByMouse: true
                activeFocusOnTab: false
                wrapMode: Text.Wrap
                textFormat: TextEdit.PlainText
                font.family: cell.entry.mono === true ? TelamonStyle.monoFamily : TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
                selectionColor: TelamonStyle.accent
                selectedTextColor: TelamonStyle.accentText
                Accessible.role: Accessible.StaticText
                Accessible.name: String(cell.entry.label ?? "") + ": " + cell.text
            }
            TelamonCopyButton {
                id: copy
                visible: cell.isValue && cell.entry.copyable === true
                anchors.right: parent.right
                anchors.top: parent.top
                text: cell.text
            }
        }
    }
}
