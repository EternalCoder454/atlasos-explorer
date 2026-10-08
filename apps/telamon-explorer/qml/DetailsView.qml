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
    // Ctrl+scroll, Ctrl+plus and Ctrl+minus change the rows' height (kept); the header stays.
    readonly property int rowHeight: PreviewLogic.rowHeight
    readonly property int headerHeight: Kirigami.Units.gridUnit * 2
    readonly property int iconSide: Math.min(rowHeight - 4, Math.max(Kirigami.Units.iconSizes.smallMedium, Math.round(rowHeight * 0.62)))
    // Search results have a Path column (the folder each one is in) after the name.
    readonly property bool withPath: fv.folder.searching
    // Widths of the columns by key.
    property var widths: ({
            "name": Kirigami.Units.gridUnit * 22,
            "path": Kirigami.Units.gridUnit * 16,
            "size": Kirigami.Units.gridUnit * 7,
            "type": Kirigami.Units.gridUnit * 12,
            "modified": Kirigami.Units.gridUnit * 11
        })
    readonly property var columns: withPath ? [
        { key: "name", title: qsTr("Name"), sort: FolderModel.Name },
        { key: "path", title: qsTr("Path"), sort: -1 },
        { key: "size", title: qsTr("Size"), sort: FolderModel.Size },
        { key: "type", title: qsTr("Type"), sort: FolderModel.Type },
        { key: "modified", title: qsTr("Modified"), sort: FolderModel.Modified }
    ] : [
        { key: "name", title: qsTr("Name"), sort: FolderModel.Name },
        { key: "size", title: qsTr("Size"), sort: FolderModel.Size },
        { key: "type", title: qsTr("Type"), sort: FolderModel.Type },
        { key: "modified", title: qsTr("Modified"), sort: FolderModel.Modified }
    ]
    readonly property real pathWidth: withPath ? widths.path : 0
    readonly property real totalWidth: widths.name + pathWidth + widths.size + widths.type + widths.modified
    readonly property int pageRows: Math.max(1, Math.floor(list.height / rowHeight) - 1)

    function reveal(row) {
        list.positionViewAtIndex(row, ListView.Contain);
    }

    // The row at a point in this item's coordinates, -1 for none (a group's
    // header is above the row it comes before, and is not a row).
    function rowAt(x, y) {
        const cy = y + list.contentY;
        const index = list.indexAt(x + list.contentX, cy);
        if (index < 0) {
            return -1;
        }
        const item = list.itemAtIndex(index);
        if (item && (cy < item.y || cy >= item.y + item.height)) {
            return -1;
        }
        return index;
    }

    // Where a row is, in the folder view's coordinates (the row may be off screen).
    function rowRect(row) {
        const item = list.itemAtIndex(row);
        if (item) {
            const at = item.mapToItem(root.fv, 0, 0);
            return Qt.rect(at.x, at.y, list.width, rowHeight);
        }
        // Not made yet: worked out from where the list is.
        const at = list.mapToItem(root.fv, 0, row * rowHeight - list.contentY);
        return Qt.rect(at.x, at.y, list.width, rowHeight);
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

        // Group by: the rows of a group are together, each group starts with a header.
        // (Not grouped, every row's group is empty and the header takes no room. The
        // property stays as it is: changing it while rows are shown crashes the list.)
        section.property: "groupKey"
        section.criteria: ViewSection.FullString
        section.delegate: GroupHeader {
            required property string section
            width: Math.max(list.width, root.totalWidth)
            fv: root.fv
            label: section
            // Read again when the groups are worked out again.
            count: {
                root.fv.folder.groupRevision;
                return root.fv.folder.groupCount(section);
            }
            collapsed: {
                root.fv.folder.groupRevision;
                return root.fv.folder.isGroupCollapsed(section);
            }
            visible: section.length > 0
            height: visible ? implicitHeight : 0
        }

        header: Rectangle {
            z: 2
            width: Math.max(list.width, root.totalWidth)
            height: root.headerHeight
            color: Kirigami.Theme.backgroundColor
            Row {
                Repeater {
                    model: root.columns
                    Item {
                        id: cell
                        required property var modelData
                        required property int index
                        width: root.widths[modelData.key]
                        height: root.headerHeight
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
                            enabled: cell.modelData.sort >= 0
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
                                startWidth = root.widths[cell.modelData.key];
                            }
                            onPositionChanged: mouse => {
                                if (!pressed) {
                                    return;
                                }
                                const w = Object.assign({}, root.widths);
                                w[cell.modelData.key] = Math.max(Kirigami.Units.gridUnit * 4, startWidth + mapToItem(null, mouse.x, 0).x - startX);
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
            required property bool isCut
            required property string sizeText
            required property string modifiedText
            required property string typeText
            required property string pathText
            required property bool groupCollapsed
            width: Math.max(list.width, root.totalWidth)
            // The rows of a collapsed group take no room.
            height: groupCollapsed ? 0 : root.rowHeight
            visible: !groupCollapsed
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
                opacity: (row.isHidden ? 0.6 : 1) * (row.isCut ? 0.5 : 1)
                Item {
                    width: root.widths.name
                    height: root.rowHeight
                    Kirigami.Icon {
                        id: icon
                        x: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        width: root.iconSide
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
                    width: root.pathWidth
                    height: root.rowHeight
                    visible: root.withPath
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    text: row.pathText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
                Text {
                    width: root.widths.size
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    text: row.sizeText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
                Text {
                    width: root.widths.type
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    text: row.typeText
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.75)
                }
                Text {
                    width: root.widths.modified
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
                acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
                property point start
                property bool narrow: false
                property bool dragged: false
                onPressed: mouseEvent => {
                    if (mouseEvent.button === Qt.RightButton) {
                        const at = mouse.mapToItem(root.fv, mouseEvent.x, mouseEvent.y);
                        root.fv.rowMenu(row.index, at.x, at.y);
                        return;
                    }
                    if (mouseEvent.button === Qt.MiddleButton) {
                        root.fv.middleRow(row.index);
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
