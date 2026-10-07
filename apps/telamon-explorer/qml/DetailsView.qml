pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Details view: one fixed-height row per item, a header with sortable and
// resizable columns. Only the rows on screen exist.
Item {
    id: root

    required property var fv
    readonly property int rowHeight: Kirigami.Units.gridUnit * 2
    // Widths of Name, Size, Type and Modified.
    property var widths: [Kirigami.Units.gridUnit * 22, Kirigami.Units.gridUnit * 7, Kirigami.Units.gridUnit * 12, Kirigami.Units.gridUnit * 11]
    readonly property var columns: [
        { title: qsTr("Name"), sort: FolderModel.Name },
        { title: qsTr("Size"), sort: FolderModel.Size },
        { title: qsTr("Type"), sort: FolderModel.Type },
        { title: qsTr("Modified"), sort: FolderModel.Modified }
    ]
    readonly property real totalWidth: widths.reduce((a, b) => a + b, 0)
    readonly property int pageRows: Math.max(1, Math.floor(list.height / rowHeight) - 1)

    function reveal(row) {
        list.positionViewAtIndex(row, ListView.Contain);
    }

    // The row at a point in this item's coordinates, -1 for none.
    function rowAt(x, y) {
        return list.indexAt(x + list.contentX, y + list.contentY);
    }

    function neighbor(row, dir) {
        switch (dir) {
        case "up":
        case "left":
            return row - 1;
        case "down":
        case "right":
            return row + 1;
        case "pageUp":
            return row - pageRows;
        default:
            return row + pageRows;
        }
    }

    ListView {
        id: list
        anchors.fill: parent
        model: root.visible ? root.fv.folder : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentWidth: root.totalWidth
        flickableDirection: Flickable.HorizontalAndVerticalFlick
        headerPositioning: ListView.OverlayHeader
        cacheBuffer: root.rowHeight * 8
        QQC2.ScrollBar.vertical: TelamonScrollBar {}
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}

        header: Rectangle {
            z: 2
            width: Math.max(list.width, root.totalWidth)
            height: root.rowHeight
            color: Kirigami.Theme.backgroundColor
            Row {
                Repeater {
                    model: root.columns
                    Item {
                        id: cell
                        required property var modelData
                        required property int index
                        width: root.widths[index]
                        height: root.rowHeight
                        readonly property bool sorted: root.fv.folder.sortColumn === modelData.sort
                        Text {
                            anchors.fill: parent
                            anchors.leftMargin: Kirigami.Units.largeSpacing
                            anchors.rightMargin: Kirigami.Units.gridUnit * 1.5
                            verticalAlignment: Text.AlignVCenter
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            text: cell.modelData.title + (cell.sorted ? (root.fv.folder.sortDescending ? "  ▼" : "  ▲") : "")
                            color: Kirigami.Theme.textColor
                            font.bold: cell.sorted
                        }
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {
                                const f = root.fv.folder;
                                if (f.sortColumn === cell.modelData.sort) {
                                    f.sortDescending = !f.sortDescending;
                                } else {
                                    f.sortColumn = cell.modelData.sort;
                                    f.sortDescending = false;
                                }
                            }
                        }
                        Rectangle {
                            anchors.right: parent.right
                            width: 1
                            height: parent.height
                            color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
                        }
                        MouseArea {
                            anchors.right: parent.right
                            width: Kirigami.Units.largeSpacing
                            height: parent.height
                            cursorShape: Qt.SizeHorCursor
                            property real startX
                            property real startWidth
                            onPressed: mouse => {
                                startX = mapToItem(null, mouse.x, 0).x;
                                startWidth = root.widths[cell.index];
                            }
                            onPositionChanged: mouse => {
                                if (!pressed) {
                                    return;
                                }
                                const w = root.widths.slice();
                                w[cell.index] = Math.max(Kirigami.Units.gridUnit * 4, startWidth + mapToItem(null, mouse.x, 0).x - startX);
                                root.widths = w;
                            }
                        }
                    }
                }
            }
        }

        delegate: Item {
            id: row
            required property int index
            required property string name
            required property string iconName
            required property bool isDir
            required property bool isHidden
            required property string sizeText
            required property string modifiedText
            required property string typeText
            width: Math.max(list.width, root.totalWidth)
            height: root.rowHeight
            readonly property bool selected: root.fv.isSelected(index, root.fv.selRevision)
            readonly property bool current: root.fv.currentRow === index

            Rectangle {
                anchors.fill: parent
                anchors.margins: 1
                radius: 4
                color: row.selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.35) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
                border.width: row.current && root.fv.activeFocus ? 1 : 0
                border.color: Kirigami.Theme.highlightColor
            }
            Row {
                opacity: row.isHidden ? 0.6 : 1
                Item {
                    width: root.widths[0]
                    height: root.rowHeight
                    Kirigami.Icon {
                        id: icon
                        x: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        width: Kirigami.Units.iconSizes.smallMedium
                        height: width
                        source: row.iconName
                    }
                    Text {
                        anchors.left: icon.right
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.right: parent.right
                        anchors.rightMargin: Kirigami.Units.largeSpacing
                        height: parent.height
                        verticalAlignment: Text.AlignVCenter
                        textFormat: Text.PlainText
                        elide: Text.ElideMiddle
                        text: row.name
                        color: Kirigami.Theme.textColor
                    }
                }
                Text {
                    width: root.widths[1]
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    text: row.sizeText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
                Text {
                    width: root.widths[2]
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    text: row.typeText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
                Text {
                    width: root.widths[3]
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    text: row.modifiedText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
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
                        root.fv.rowMenu(row.index);
                        return;
                    }
                    start = Qt.point(mouseEvent.x, mouseEvent.y);
                    dragged = false;
                    narrow = !root.fv.pressRow(row.index, mouseEvent.modifiers);
                }
                onPositionChanged: mouseEvent => {
                    if (pressed && !dragged && (pressedButtons & Qt.LeftButton) && Math.hypot(mouseEvent.x - start.x, mouseEvent.y - start.y) > Application.styleHints.startDragDistance) {
                        dragged = true;
                        root.fv.beginDrag();
                    }
                }
                onReleased: mouseEvent => {
                    if (narrow && !dragged) {
                        root.fv.chooseRow(row.index, 0, false);
                    }
                    narrow = false;
                }
                onDoubleClicked: root.fv.activateRow(row.index)
            }
        }
    }
}
