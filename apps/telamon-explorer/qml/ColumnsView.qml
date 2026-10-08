pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Columns view: the folders on the way to the tab's folder, side by side,
// then the tab's folder (whose items are selected as in the other views), then
// what the selected item is: a folder opens as the next column, a file is
// shown as a preview (the preview pane's). Right arrow goes into a folder,
// Left comes back out; Space is Quick Look.
Item {
    id: root

    required property var fv
    readonly property real columnWidth: Kirigami.Units.gridUnit * 15
    readonly property real previewWidth: Kirigami.Units.gridUnit * 20
    // The first column's folder and the folders after it up to the tab's.
    property url rootUrl
    property var ancestors: []

    // The row selected when exactly one item is, else -1.
    readonly property int onlyRow: {
        fv.selRevision;
        fv.folder.count;
        const rows = fv.selectedRows();
        return rows.length === 1 ? rows[0] : -1;
    }
    readonly property bool childIsFolder: onlyRow >= 0 && fv.folder.isDirAt(onlyRow)
    readonly property url childUrl: childIsFolder ? fv.folder.urlAt(onlyRow) : Qt.url("")

    function same(a, b) {
        return a.toString().replace(/\/+$/, "") === b.toString().replace(/\/+$/, "");
    }
    // The columns before the tab's: its parents back to the first column.
    // A folder that isn't below the first column starts new columns.
    function rebuild() {
        const here = fv.folder.url;
        if (here.toString().length === 0) {
            return;
        }
        const up = [];
        let u = here;
        let ok = same(u, rootUrl);
        while (!ok && up.length < 64) {
            const p = StandardPlaces.parentUrl(u);
            if (same(p, u)) {
                break;
            }
            up.unshift(p);
            u = p;
            ok = same(u, rootUrl);
        }
        if (!ok) {
            rootUrl = here;
            ancestors = [];
        } else {
            ancestors = up;
        }
        Qt.callLater(root.scrollToEnd);
    }
    onVisibleChanged: {
        if (visible) {
            rootUrl = fv.folder.url;
            rebuild();
        }
    }
    Connections {
        target: root.fv.folder
        function onUrlChanged() {
            if (root.visible) {
                root.rebuild();
            }
        }
    }

    // The last columns are the ones in view.
    function scrollToEnd() {
        strip.contentX = Math.max(0, strip.contentWidth - strip.width);
    }

    function reveal(row) {
        primaryColumn.reveal(row);
    }
    // The primary column's row at a point in this view's coordinates, -1 for none.
    function rowAt(x, y) {
        const at = root.mapToItem(primaryColumn, x, y);
        if (at.x < 0 || at.x >= primaryColumn.width || at.y < 0 || at.y >= primaryColumn.height) {
            return -1;
        }
        return primaryColumn.rowAt(at.x, at.y);
    }
    function rowRect(row) {
        const at = primaryColumn.mapToItem(root.fv, 0, primaryColumn.rowY(row));
        return Qt.rect(at.x, at.y, primaryColumn.width, primaryColumn.rowHeight);
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
            return row - primaryColumn.pageRows;
        default:
            return row + primaryColumn.pageRows;
        }
    }

    Flickable {
        id: strip
        anchors.fill: parent
        clip: true
        contentWidth: columnsRow.width
        contentHeight: height
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.HorizontalFlick
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}
        onWidthChanged: Qt.callLater(root.scrollToEnd)
        onContentWidthChanged: Qt.callLater(root.scrollToEnd)

        Row {
            id: columnsRow
            height: strip.height

            Repeater {
                model: root.ancestors
                delegate: ColumnList {
                    required property var modelData
                    required property int index
                    width: root.columnWidth
                    height: columnsRow.height
                    fv: root.fv
                    folderUrl: modelData
                    highlightUrl: index + 1 < root.ancestors.length ? root.ancestors[index + 1] : root.fv.folder.url
                }
            }
            ColumnList {
                id: primaryColumn
                width: root.columnWidth
                height: columnsRow.height
                fv: root.fv
                primary: true
            }
            // What the selected item is: a folder's items, or the preview of a file.
            Item {
                width: root.childIsFolder ? root.columnWidth : root.previewWidth
                height: columnsRow.height

                Loader {
                    anchors.fill: parent
                    active: root.childIsFolder
                    sourceComponent: ColumnList {
                        fv: root.fv
                        folderUrl: root.childUrl
                    }
                }
                PreviewPane {
                    anchors.fill: parent
                    visible: !root.childIsFolder
                    view: root.visible ? root.fv : null
                    covered: root.fv.covered || PreviewLogic.paneShown
                }
            }
        }
    }
}
