pragma ComponentBehavior: Bound
import QtQuick
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The header of a group of items in the Details and Icons views (Group by):
// a chevron, the group's name and how many items it has. A click folds the
// group's items away or brings them back.
Item {
    id: header

    // The FolderView.
    required property var fv
    property string label
    property int count: 0
    property bool collapsed: false

    implicitHeight: Kirigami.Units.gridUnit * 2

    // The core names the date groups in English; the window says them in its language.
    function groupText(name) {
        switch (name) {
        case "Today":
            return qsTr("Today");
        case "Yesterday":
            return qsTr("Yesterday");
        case "Earlier This Week":
            return qsTr("Earlier This Week");
        case "Last Week":
            return qsTr("Last Week");
        case "Earlier This Month":
            return qsTr("Earlier This Month");
        case "Last Month":
            return qsTr("Last Month");
        case "Earlier This Year":
            return qsTr("Earlier This Year");
        case "A Long Time Ago":
            return qsTr("A Long Time Ago");
        case "Later":
            return qsTr("Later");
        case "Unknown":
            return qsTr("Unknown");
        case "Other":
            return qsTr("Other");
        default:
            return name;
        }
    }
    readonly property string countText: count === 1 ? qsTr("1 item") : qsTr("%1 items").arg(count)

    Accessible.role: Accessible.Button
    Accessible.name: groupText(label) + ", " + countText + ", " + (collapsed ? qsTr("collapsed") : qsTr("expanded"))
    Accessible.onPressAction: header.fv.toggleGroup(header.label)

    Rectangle {
        anchors.fill: parent
        anchors.margins: 1
        radius: TelamonStyle.radiusSmall
        color: area.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent"
    }
    Row {
        anchors.left: parent.left
        anchors.leftMargin: Kirigami.Units.largeSpacing
        anchors.verticalCenter: parent.verticalCenter
        spacing: Kirigami.Units.largeSpacing
        Symbol {
            anchors.verticalCenter: parent.verticalCenter
            icon: header.collapsed ? Symbols.ChevronRight : Symbols.ExpandMore
            size: Kirigami.Units.iconSizes.small
            color: Kirigami.Theme.textColor
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            font.bold: true
            color: Kirigami.Theme.textColor
            text: header.groupText(header.label)
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
            text: header.countText
        }
    }
    Rectangle {
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Kirigami.Units.largeSpacing
        height: 1
        color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        onClicked: header.fv.toggleGroup(header.label)
    }
}
