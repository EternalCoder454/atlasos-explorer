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
    // The rows' height is the same setting as in the Details view.
    readonly property int icon: compact ? Math.min(PreviewLogic.rowHeight - 4, Math.max(Kirigami.Units.iconSizes.smallMedium, Math.round(PreviewLogic.rowHeight * 0.62))) : fv.iconSize
    readonly property real cellW: compact ? Kirigami.Units.gridUnit * 14 : Math.max(icon + Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 6)
    readonly property real cellH: compact ? PreviewLogic.rowHeight : icon + Kirigami.Units.gridUnit * 3.2
    readonly property int perRow: Math.max(1, Math.floor(grid.width / cellW))
    readonly property int perColumn: Math.max(1, Math.floor(grid.height / cellH))

    // Group by: lines of cells and headers in a list (a grid can't have a header
    // across its width). Off, the folder is one grid.
    readonly property bool grouped: !compact && fv.folder.grouped

    function reveal(row) {
        if (grouped) {
            const line = lines.lineOf(row);
            if (line >= 0) {
                list.positionViewAtIndex(line, ListView.Contain);
            }
        } else {
            grid.positionViewAtIndex(row, GridView.Contain);
        }
    }

    // The row at a point in this item's coordinates, -1 for none.
    function rowAt(x, y) {
        if (!grouped) {
            return grid.indexAt(x + grid.contentX, y + grid.contentY);
        }
        const cy = y + list.contentY;
        const line = list.indexAt(0, cy);
        const item = line >= 0 ? list.itemAtIndex(line) : null;
        if (!item || cy < item.y || cy >= item.y + item.height) {
            return -1;
        }
        return lines.rowAtCell(line, Math.floor(x / cellW));
    }

    // Where a row is, in the folder view's coordinates (the row may be off screen).
    function rowRect(row) {
        if (grouped) {
            const line = lines.lineOf(row);
            const item = line >= 0 ? list.itemAtIndex(line) : null;
            if (item) {
                const at = item.mapToItem(root.fv, ((row - item.first) * cellW), 0);
                return Qt.rect(at.x, at.y, cellW, cellH);
            }
            const at = list.mapToItem(root.fv, 0, -list.contentY);
            return Qt.rect(at.x, at.y, cellW, cellH);
        }
        const item = grid.itemAtIndex(row);
        if (item) {
            const at = item.mapToItem(root.fv, 0, 0);
            return Qt.rect(at.x, at.y, cellW, cellH);
        }
        // Not made yet: worked out from where the grid is.
        const along = compact ? perColumn : perRow;
        const line = Math.floor(row / along);
        const place = row % along;
        const column = compact ? line : place;
        const rowInColumn = compact ? place : line;
        const at = grid.mapToItem(root.fv, column * cellW - grid.contentX, rowInColumn * cellH - grid.contentY);
        return Qt.rect(at.x, at.y, cellW, cellH);
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
        visible: !root.grouped
        model: root.visible && !root.grouped ? root.fv.folder : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flow: root.compact ? GridView.FlowTopToBottom : GridView.FlowLeftToRight
        cellWidth: root.cellW
        cellHeight: root.cellH
        cacheBuffer: root.cellH * 4
        QQC2.ScrollBar.vertical: TelamonScrollBar {}
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}

        delegate: IconCell {
            required property int index
            view: root
            row: index
        }
    }

    GroupLines {
        id: lines
        source: root.visible && root.grouped ? root.fv.folder : null
        perRow: root.perRow
    }

    ListView {
        id: list
        anchors.fill: parent
        visible: root.grouped
        // The lines are empty while the folder isn't grouped.
        model: lines
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: root.cellH * 4
        QQC2.ScrollBar.vertical: TelamonScrollBar {}

        delegate: Item {
            id: line
            required property int kind
            required property string label
            required property int count
            required property bool collapsed
            required property int first
            required property int cells
            width: list.width
            height: kind === 0 ? header.implicitHeight : root.cellH

            GroupHeader {
                id: header
                anchors.fill: parent
                visible: line.kind === 0
                fv: root.fv
                label: line.label
                count: line.count
                collapsed: line.collapsed
            }
            Loader {
                active: line.kind === 1
                sourceComponent: Row {
                    Repeater {
                        model: RowSlice {
                            source: root.fv.folder
                            first: line.first
                            count: line.cells
                        }
                        delegate: IconCell {
                            required property int sourceRow
                            view: root
                            row: sourceRow
                        }
                    }
                }
            }
        }
    }
}
