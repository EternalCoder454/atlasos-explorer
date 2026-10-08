pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The header of the Trash page: the "Empty items older than N days" switch,
// above the items. Files empties only what is older, never what is newer.
Rectangle {
    id: bar

    implicitHeight: content.implicitHeight + TelamonStyle.spacing * 2
    color: Qt.alpha(Kirigami.Theme.textColor, 0.04)

    RowLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Kirigami.Units.largeSpacing * 2
        anchors.rightMargin: Kirigami.Units.largeSpacing * 2
        spacing: TelamonStyle.spacingLarge

        TrashAutoEmpty {}
        Item {
            Layout.fillWidth: true
        }
        Text {
            // Too narrow for both: the switch matters more.
            visible: bar.width > Kirigami.Units.gridUnit * 48
            text: qsTr("Items newer than that are never touched.")
            textFormat: Text.PlainText
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            color: TelamonStyle.textMuted
        }
    }
    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
    }
}
