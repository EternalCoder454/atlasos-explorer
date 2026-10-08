pragma ComponentBehavior: Bound
import QtQuick
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One item of the Icons view (an icon or a thumbnail with its name under it)
// or of the compact list (a small icon with its name beside it). The cell
// knows its row in the folder; the view it is in (IconsView) says how big it is.
Item {
    id: cell

    // The IconsView: the cell's size and mode.
    required property var view
    // The row in the folder.
    property int row: -1
    required property url url
    required property bool isDir
    required property string name
    required property string iconName
    required property bool isHidden
    required property bool isCut
    required property string thumbnailSource

    width: cell.view.cellW
    height: cell.view.cellH
    readonly property var fv: cell.view.fv
    readonly property bool selected: fv.isSelected(row, fv.selRevision)
    readonly property bool current: fv.currentRow === row
    // The name is being edited where it is shown.
    readonly property bool editing: fv.renaming && fv.renameUrl.toString() === url.toString()

    Rectangle {
        anchors.fill: parent
        anchors.margins: 2
        radius: TelamonStyle.radius
        color: cell.selected ? Qt.alpha(Kirigami.Theme.highlightColor, 0.35) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
        border.width: cell.current && cell.fv.activeFocus ? 1 : 0
        border.color: Kirigami.Theme.highlightColor
    }
    Item {
        id: iconBox
        x: cell.view.compact ? Kirigami.Units.largeSpacing : (parent.width - width) / 2
        y: cell.view.compact ? (parent.height - height) / 2 : Kirigami.Units.smallSpacing * 2
        width: cell.view.icon
        height: cell.view.icon
        opacity: (cell.isHidden ? 0.6 : 1) * (cell.isCut ? 0.5 : 1)
        Kirigami.Icon {
            anchors.fill: parent
            source: cell.iconName
            // The file's own picture takes its place.
            visible: !thumb.visible
        }
        Image {
            id: thumb
            anchors.fill: parent
            // Thumbnails only in the Icons view; a file without one
            // comes back 1x1 and the icon stays.
            source: !cell.view.compact && cell.thumbnailSource.length > 0 ? cell.thumbnailSource : ""
            sourceSize: Qt.size(cell.view.icon * Screen.devicePixelRatio, cell.view.icon * Screen.devicePixelRatio)
            fillMode: Image.PreserveAspectFit
            asynchronous: true
            cache: false
            visible: status === Image.Ready && implicitWidth > 1
        }
    }
    Text {
        x: cell.view.compact ? iconBox.x + iconBox.width + Kirigami.Units.largeSpacing : Kirigami.Units.smallSpacing
        y: cell.view.compact ? 0 : iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
        width: cell.width - x - Kirigami.Units.smallSpacing
        height: cell.view.compact ? cell.height : cell.height - y
        verticalAlignment: cell.view.compact ? Text.AlignVCenter : Text.AlignTop
        horizontalAlignment: cell.view.compact ? Text.AlignLeft : Text.AlignHCenter
        visible: !cell.editing
        textFormat: Text.PlainText
        wrapMode: cell.view.compact ? Text.NoWrap : Text.WrapAnywhere
        maximumLineCount: cell.view.compact ? 1 : 2
        elide: Text.ElideRight
        text: cell.name
        color: Kirigami.Theme.textColor
    }
    Loader {
        x: cell.view.compact ? iconBox.x + iconBox.width + Kirigami.Units.largeSpacing : Kirigami.Units.smallSpacing
        y: cell.view.compact ? (cell.height - height) / 2 : iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
        width: cell.width - x - Kirigami.Units.smallSpacing
        active: cell.editing
        z: 2
        sourceComponent: InlineRename {
            fv: cell.fv
            itemUrl: cell.url
            isDir: cell.isDir
            centered: !cell.view.compact
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
                const at = mouse.mapToItem(cell.fv, mouseEvent.x, mouseEvent.y);
                cell.fv.rowMenu(cell.row, at.x, at.y);
                return;
            }
            if (mouseEvent.button === Qt.MiddleButton) {
                cell.fv.middleRow(cell.row);
                return;
            }
            start = Qt.point(mouseEvent.x, mouseEvent.y);
            dragged = false;
            narrow = !cell.fv.pressRow(cell.row, mouseEvent.modifiers);
        }
        onPositionChanged: mouseEvent => {
            if (pressed && !dragged && (pressedButtons & Qt.LeftButton) && Math.hypot(mouseEvent.x - start.x, mouseEvent.y - start.y) > Application.styleHints.startDragDistance) {
                dragged = true;
                cell.fv.beginDrag();
            }
        }
        onReleased: mouseEvent => {
            if (narrow && !dragged) {
                cell.fv.chooseRow(cell.row, 0, false);
            }
            narrow = false;
        }
        onDoubleClicked: cell.fv.activateRow(cell.row)
    }
}
