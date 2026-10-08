pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Gallery view: the item with the keyboard shown large (a picture, a
// player, text, as in the preview pane) over a filmstrip of the folder's
// items. The arrow keys move along the filmstrip; Space is Quick Look and
// Enter opens, as in the other views.
Item {
    id: root

    required property var fv
    readonly property real thumbSide: Kirigami.Units.gridUnit * 5
    readonly property real cellW: thumbSide + Kirigami.Units.gridUnit
    readonly property real cellH: thumbSide + Kirigami.Units.gridUnit * 2.4
    readonly property int perPage: Math.max(1, Math.floor(strip.width / cellW) - 1)

    // What the large preview shows: the row the keyboard is on.
    property var info: ({})
    function refresh() {
        const f = fv.folder;
        info = fv.currentRow >= 0 && fv.currentRow < f.count ? f.detailsAt(fv.currentRow) : ({});
    }
    Connections {
        target: root.fv
        function onCurrentRowChanged() {
            root.refresh();
        }
    }
    Connections {
        target: root.fv.folder
        function onModelReset() {
            root.refresh();
            root.start();
        }
        function onLayoutChanged() {
            root.refresh();
        }
        function onDataChanged() {
            root.refresh();
        }
        function onLoadingChanged() {
            root.start();
        }
    }
    // The view starts on an item: the first when the keyboard isn't on one.
    function start() {
        if (visible && fv.currentRow < 0 && fv.folder.count > 0 && !fv.folder.loading) {
            const first = fv.folder.visibleRowFrom(0, 1);
            if (first >= 0) {
                fv.chooseRow(first, 0, false);
            }
        }
    }
    onVisibleChanged: {
        if (visible) {
            start();
            refresh();
        }
    }
    Component.onCompleted: refresh()

    function reveal(row) {
        strip.positionViewAtIndex(row, ListView.Contain);
    }
    // The filmstrip's item at a point in this view's coordinates, -1 for none.
    function rowAt(x, y) {
        const at = root.mapToItem(strip, x, y);
        if (at.x < 0 || at.y < 0 || at.x >= strip.width || at.y >= strip.height) {
            return -1;
        }
        return strip.indexAt(at.x + strip.contentX, at.y + strip.contentY);
    }
    function rowRect(row) {
        const item = strip.itemAtIndex(row);
        const at = item ? item.mapToItem(root.fv, 0, 0) : strip.mapToItem(root.fv, row * cellW - strip.contentX, 0);
        return Qt.rect(at.x, at.y, cellW, cellH);
    }
    // Left and Up are the one before, Right and Down the one after.
    function neighbor(row, dir) {
        switch (dir) {
        case "up":
        case "left":
            return row - 1;
        case "down":
        case "right":
            return row + 1;
        case "pageUp":
            return row - perPage;
        default:
            return row + perPage;
        }
    }

    PreviewBody {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: caption.top
        anchors.margins: Kirigami.Units.largeSpacing
        info: root.info
        playerActive: root.visible && !root.fv.covered && !PreviewLogic.paneShown
        thumbSide: 1600
        iconSide: Kirigami.Units.iconSizes.enormous
    }

    // Where the item is in the folder, and its name.
    Text {
        id: caption
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: strip.top
        anchors.leftMargin: Kirigami.Units.gridUnit
        anchors.rightMargin: Kirigami.Units.gridUnit
        height: Kirigami.Units.gridUnit * 1.8
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignHCenter
        textFormat: Text.PlainText
        elide: Text.ElideMiddle
        color: Kirigami.Theme.textColor
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        text: root.fv.currentRow >= 0 && root.info.name !== undefined ? qsTr("%1 (%2 of %3)").arg(root.info.name).arg(root.fv.currentRow + 1).arg(root.fv.folder.count) : ""
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: strip.top
        height: 1
        color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
    }

    ListView {
        id: strip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: root.cellH + Kirigami.Units.smallSpacing * 2
        orientation: ListView.Horizontal
        model: root.visible ? root.fv.folder : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: root.cellW * 6
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}

        delegate: Item {
            id: cell
            required property int index
            required property string name
            required property string iconName
            required property bool isHidden
            required property bool isCut
            required property string thumbnailSource
            required property var tagColours
            required property string typeText
            required property string sizeText
            required property int gitBadge
            width: root.cellW
            height: strip.height
            readonly property bool selected: root.fv.isSelected(index, root.fv.selRevision)
            readonly property bool current: root.fv.currentRow === index

            // What a screen reader says: the name, then the type, size and Git state, and whether the item is selected.
            Accessible.role: Accessible.ListItem
            Accessible.name: cell.name
            Accessible.description: root.fv.rowDescription(cell.typeText, cell.sizeText, cell.gitBadge)
            Accessible.selectable: true
            Accessible.selected: cell.selected
            Accessible.focusable: true
            Accessible.focused: cell.current && root.fv.activeFocus

            Rectangle {
                anchors.fill: parent
                anchors.margins: 2
                radius: TelamonStyle.radius
                color: cell.selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.35) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
                border.width: cell.current ? (root.fv.activeFocus ? 2 : 1) : 0
                border.color: Kirigami.Theme.highlightColor
            }
            Item {
                id: iconBox
                x: (parent.width - width) / 2
                y: Kirigami.Units.smallSpacing * 2
                width: root.thumbSide
                height: root.thumbSide
                opacity: (cell.isHidden ? 0.6 : 1) * (cell.isCut ? 0.5 : 1)
                Kirigami.Icon {
                    anchors.fill: parent
                    source: cell.iconName
                    visible: !thumb.visible
                }
                Image {
                    id: thumb
                    anchors.fill: parent
                    source: cell.thumbnailSource
                    sourceSize: Qt.size(root.thumbSide * Screen.devicePixelRatio, root.thumbSide * Screen.devicePixelRatio)
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    cache: false
                    visible: status === Image.Ready && implicitWidth > 1
                }
            }
            TagDots {
                x: iconBox.x + iconBox.width - width + Math.round(dot * 0.4)
                y: iconBox.y + iconBox.height - height + Math.round(dot * 0.2)
                colours: cell.tagColours
                dot: 12
            }
            GitBadge {
                x: iconBox.x - Math.round(dot * 0.3)
                y: iconBox.y + iconBox.height - height + Math.round(dot * 0.2)
                code: cell.gitBadge
                dot: 14
            }
            Text {
                x: Kirigami.Units.smallSpacing
                y: iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
                width: cell.width - 2 * x
                height: cell.height - y
                horizontalAlignment: Text.AlignHCenter
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                maximumLineCount: 1
                text: cell.name
                color: Kirigami.Theme.textColor
                font.pointSize: TelamonStyle.fontSizeCaption
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
                        root.fv.rowMenu(cell.index, at.x, at.y);
                        return;
                    }
                    if (mouseEvent.button === Qt.MiddleButton) {
                        root.fv.middleRow(cell.index);
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
