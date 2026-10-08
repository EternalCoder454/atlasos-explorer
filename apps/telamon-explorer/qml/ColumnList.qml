pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One column of the Columns view: a folder's items in a list, a chevron on
// the folders. The column of the tab's own folder (`primary`) is the one with
// the selection and the keyboard: its rows behave as in the other views. The
// others are the folders above it (with the item on the way to the tab's
// folder marked) and the folder selected in the primary column; clicking an
// item in one goes to that column's folder with the item selected.
Item {
    id: col

    // The FolderView.
    required property var fv
    property bool primary: false
    // The folder of a column that isn't the primary, and the item in it that
    // leads to the next column.
    property url folderUrl
    property url highlightUrl

    readonly property var folderModel: primary ? fv.folder : own.item
    readonly property int rowHeight: PreviewLogic.rowHeight
    readonly property int iconSide: Math.min(rowHeight - 4, Math.max(Kirigami.Units.iconSizes.smallMedium, Math.round(rowHeight * 0.62)))
    // Bumped when the rows moved, so the marked row is found again.
    property int layoutRevision: 0
    readonly property int highlightRow: {
        col.layoutRevision;
        if (primary || !folderModel || highlightUrl.toString().length === 0) {
            return -1;
        }
        folderModel.count;
        return folderModel.rowOfUrl(highlightUrl);
    }
    onHighlightRowChanged: {
        if (highlightRow >= 0) {
            list.positionViewAtIndex(highlightRow, ListView.Contain);
        }
    }

    // The folder of a column that isn't the tab's: listed like the tab's, with its sort.
    Loader {
        id: own
        active: !col.primary
        sourceComponent: FolderModel {
            url: col.folderUrl
            showHidden: col.fv.folder.showHidden
            sortColumn: col.fv.folder.sortColumn
            sortDescending: col.fv.folder.sortDescending
            foldersFirst: col.fv.folder.foldersFirst
        }
    }
    Connections {
        target: col.folderModel
        function onLayoutChanged() {
            col.layoutRevision++;
        }
        function onModelReset() {
            col.layoutRevision++;
        }
    }

    function reveal(row) {
        list.positionViewAtIndex(row, ListView.Contain);
    }
    // The row at a point in this column's coordinates, -1 for none.
    function rowAt(x, y) {
        return list.indexAt(x + list.contentX, y + list.contentY);
    }
    function rowY(row) {
        const item = list.itemAtIndex(row);
        return item ? item.y - list.contentY : row * rowHeight - list.contentY;
    }
    readonly property int pageRows: Math.max(1, Math.floor(list.height / rowHeight) - 1)

    Rectangle {
        anchors.right: parent.right
        width: 1
        height: parent.height
        color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
    }

    ListView {
        id: list
        anchors.fill: parent
        anchors.rightMargin: 1
        model: col.visible ? col.folderModel : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: col.rowHeight * 8
        QQC2.ScrollBar.vertical: TelamonScrollBar {}

        delegate: Item {
            id: row
            required property int index
            required property url url
            required property string name
            required property string iconName
            required property bool isDir
            required property bool isHidden
            required property bool isCut
            width: list.width
            height: col.rowHeight
            readonly property bool selected: col.primary && col.fv.isSelected(index, col.fv.selRevision)
            readonly property bool picked: !col.primary && index === col.highlightRow
            readonly property bool current: col.primary && col.fv.currentRow === index

            Rectangle {
                anchors.fill: parent
                anchors.margins: 1
                radius: TelamonStyle.radiusSmall
                color: {
                    if (row.selected) {
                        return Qt.alpha(Kirigami.Theme.highlightColor, 0.35);
                    }
                    if (row.picked) {
                        return Qt.alpha(Kirigami.Theme.textColor, 0.16);
                    }
                    return mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent";
                }
                border.width: row.current && col.fv.activeFocus ? 1 : 0
                border.color: Kirigami.Theme.highlightColor
            }
            Row {
                anchors.fill: parent
                anchors.leftMargin: Kirigami.Units.largeSpacing
                anchors.rightMargin: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.largeSpacing
                opacity: (row.isHidden ? 0.6 : 1) * (row.isCut ? 0.5 : 1)
                Kirigami.Icon {
                    id: icon
                    anchors.verticalCenter: parent.verticalCenter
                    width: col.iconSide
                    height: width
                    source: row.iconName
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - icon.width - chevron.width - parent.spacing * 2
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    text: row.name
                    color: Kirigami.Theme.textColor
                }
                Symbol {
                    id: chevron
                    anchors.verticalCenter: parent.verticalCenter
                    width: row.isDir ? implicitWidth : 0
                    visible: row.isDir
                    icon: Symbols.ChevronRight
                    size: Kirigami.Units.iconSizes.small
                    color: Qt.alpha(Kirigami.Theme.textColor, 0.6)
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
                        const at = mouse.mapToItem(col.fv, mouseEvent.x, mouseEvent.y);
                        if (col.primary) {
                            col.fv.rowMenu(row.index, at.x, at.y);
                        } else {
                            // The column's folder is shown first: the menu is for the item there.
                            col.fv.pickInColumn(col.folderUrl, row.url);
                            col.fv.menuAfterLoad(col.folderUrl, row.url, at.x, at.y);
                        }
                        return;
                    }
                    if (mouseEvent.button === Qt.MiddleButton) {
                        if (row.isDir) {
                            col.fv.openInNewTabRequested(row.url);
                        }
                        return;
                    }
                    start = Qt.point(mouseEvent.x, mouseEvent.y);
                    dragged = false;
                    if (col.primary) {
                        narrow = !col.fv.pressRow(row.index, mouseEvent.modifiers);
                    } else {
                        col.fv.forceActiveFocus();
                        col.fv.pickInColumn(col.folderUrl, row.url);
                    }
                }
                onPositionChanged: mouseEvent => {
                    if (pressed && !dragged && (pressedButtons & Qt.LeftButton) && Math.hypot(mouseEvent.x - start.x, mouseEvent.y - start.y) > Application.styleHints.startDragDistance) {
                        dragged = true;
                        if (col.primary) {
                            col.fv.beginDrag();
                        } else if (col.fv.actions) {
                            col.fv.actions.startDrag([row.url]);
                        }
                    }
                }
                onReleased: mouseEvent => {
                    if (narrow && !dragged) {
                        col.fv.chooseRow(row.index, 0, false);
                    }
                    narrow = false;
                }
                onDoubleClicked: {
                    if (col.primary) {
                        col.fv.activateRow(row.index);
                    } else if (row.isDir) {
                        col.fv.enterColumn(row.url);
                    } else {
                        col.fv.openRequested([row.url]);
                    }
                }
            }
        }
    }

    // Files dropped on a column go into the folder under them, or into the
    // column's own folder.
    DropArea {
        anchors.fill: parent
        enabled: !col.primary
        onDropped: drop => {
            if (!col.fv.actions || !drop.hasUrls || !col.folderModel) {
                return;
            }
            const r = col.rowAt(drop.x, drop.y);
            const target = r >= 0 && col.folderModel.isDirAt(r) ? col.folderModel.urlAt(r) : col.folderUrl;
            drop.accepted = true;
            col.fv.actions.drop(drop.urls, target);
        }
    }
}
