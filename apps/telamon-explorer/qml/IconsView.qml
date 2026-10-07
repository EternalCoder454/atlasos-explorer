pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Icons view (a grid of large icons or thumbnails) and, with `compact`,
// the compact list (small icons, names beside them, filling top to bottom).
Item {
    id: root

    required property var fv
    property bool compact: false
    readonly property int icon: compact ? Kirigami.Units.iconSizes.smallMedium : fv.iconSize
    readonly property real cellW: compact ? Kirigami.Units.gridUnit * 14 : Math.max(icon + Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 6)
    readonly property real cellH: compact ? Kirigami.Units.gridUnit * 2 : icon + Kirigami.Units.gridUnit * 3.2
    readonly property int perRow: Math.max(1, Math.floor(grid.width / cellW))
    readonly property int perColumn: Math.max(1, Math.floor(grid.height / cellH))

    function reveal(row) {
        grid.positionViewAtIndex(row, GridView.Contain);
    }

    // The row at a point in this item's coordinates, -1 for none.
    function rowAt(x, y) {
        return grid.indexAt(x + grid.contentX, y + grid.contentY);
    }

    function neighbor(row, dir) {
        const page = compact ? perColumn * Math.max(1, Math.floor(grid.width / cellW) - 1) : perRow * Math.max(1, perColumn - 1);
        switch (dir) {
        case "up":
            return compact ? row - 1 : row - perRow;
        case "down":
            return compact ? row + 1 : row + perRow;
        case "left":
            return compact ? row - perColumn : row - 1;
        case "right":
            return compact ? row + perColumn : row + 1;
        case "pageUp":
            return row - page;
        default:
            return row + page;
        }
    }

    GridView {
        id: grid
        anchors.fill: parent
        model: root.visible ? root.fv.folder : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flow: root.compact ? GridView.FlowTopToBottom : GridView.FlowLeftToRight
        cellWidth: root.cellW
        cellHeight: root.cellH
        cacheBuffer: root.cellH * 4
        QQC2.ScrollBar.vertical: TelamonScrollBar {}
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}

        delegate: Item {
            id: cell
            required property int index
            required property string name
            required property string iconName
            required property bool isHidden
            required property string thumbnailSource
            width: root.cellW
            height: root.cellH
            readonly property bool selected: root.fv.isSelected(index, root.fv.selRevision)
            readonly property bool current: root.fv.currentRow === index

            Rectangle {
                anchors.fill: parent
                anchors.margins: 2
                radius: 6
                color: cell.selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.35) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
                border.width: cell.current && root.fv.activeFocus ? 1 : 0
                border.color: Kirigami.Theme.highlightColor
            }
            Item {
                id: iconBox
                x: root.compact ? Kirigami.Units.largeSpacing : (parent.width - width) / 2
                y: root.compact ? (parent.height - height) / 2 : Kirigami.Units.smallSpacing * 2
                width: root.icon
                height: root.icon
                opacity: cell.isHidden ? 0.6 : 1
                Kirigami.Icon {
                    anchors.fill: parent
                    source: cell.iconName
                }
                Image {
                    anchors.fill: parent
                    // Thumbnails only in the Icons view; a file without one
                    // comes back 1x1 and the icon stays.
                    source: !root.compact && cell.thumbnailSource.length > 0 ? cell.thumbnailSource : ""
                    sourceSize: Qt.size(root.icon * Screen.devicePixelRatio, root.icon * Screen.devicePixelRatio)
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    visible: status === Image.Ready && implicitWidth > 1
                }
            }
            Text {
                x: root.compact ? iconBox.x + iconBox.width + Kirigami.Units.largeSpacing : Kirigami.Units.smallSpacing
                y: root.compact ? 0 : iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
                width: cell.width - x - Kirigami.Units.smallSpacing
                height: root.compact ? cell.height : cell.height - y
                verticalAlignment: root.compact ? Text.AlignVCenter : Text.AlignTop
                horizontalAlignment: root.compact ? Text.AlignLeft : Text.AlignHCenter
                textFormat: Text.PlainText
                wrapMode: root.compact ? Text.NoWrap : Text.WrapAnywhere
                maximumLineCount: root.compact ? 1 : 2
                elide: Text.ElideRight
                text: cell.name
                color: Kirigami.Theme.textColor
            }
            MouseArea {
                id: mouse
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                property point start
                property bool narrow: false
                property bool dragged: false
                onPressed: mouseEvent => {
                    if (mouseEvent.button === Qt.RightButton) {
                        root.fv.rowMenu(cell.index);
                        return;
                    }
                    start = Qt.point(mouseEvent.x, mouseEvent.y);
                    dragged = false;
                    narrow = !root.fv.pressRow(cell.index, mouseEvent.modifiers);
                }
                onPositionChanged: mouseEvent => {
                    if (pressed && !dragged && (pressedButtons & Qt.LeftButton) && Math.hypot(mouseEvent.x - start.x, mouseEvent.y - start.y) > Application.styleHints.startDragDistance) {
                        dragged = true;
                        root.fv.beginDrag();
                    }
                }
                onReleased: mouseEvent => {
                    if (narrow && !dragged) {
                        root.fv.chooseRow(cell.index, 0, false);
                    }
                    narrow = false;
                }
                onDoubleClicked: root.fv.activateRow(cell.index)
            }
        }
    }
}
