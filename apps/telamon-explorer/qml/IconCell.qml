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
    required property var tagColours
    required property string typeText
    required property string sizeText
    required property int gitBadge

    width: cell.view.cellW
    height: cell.view.cellH
    readonly property var fv: cell.view.fv
    readonly property bool selected: fv.isSelected(row, fv.selRevision)
    readonly property bool current: fv.currentRow === row
    // The name is being edited where it is shown.
    readonly property bool editing: fv.renaming && fv.renameUrl.toString() === url.toString()

    // What a screen reader says: the name, then the type, size and Git state, and whether the item is selected.
    Accessible.role: Accessible.ListItem
    Accessible.name: cell.name
    Accessible.description: cell.fv.rowDescription(cell.typeText, cell.sizeText, cell.gitBadge)
    Accessible.selectable: true
    Accessible.selected: cell.selected
    Accessible.focusable: true
    Accessible.focused: cell.current && cell.fv.activeFocus

    Rectangle {
        anchors.fill: parent
        anchors.margins: 2
        radius: TelamonStyle.radius
        color: cell.selected ? (TelamonStyle.highContrast ? Kirigami.Theme.highlightColor : Qt.alpha(Kirigami.Theme.highlightColor, 0.35)) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
        border.width: cell.current && cell.fv.activeFocus ? (TelamonStyle.highContrast ? 3 : 2) : 0
        border.color: TelamonStyle.focus
    }
    // Right to left: the compact list puts the icon at the right and the name to its left.
    readonly property bool rtl: LayoutMirroring.enabled
    // What follows the name at the end of a compact row: the tag dots and the Git badge.
    readonly property real tail: cell.view.compact ? (dots.visible ? dots.width + Kirigami.Units.smallSpacing : 0) + (git.visible ? git.width + Kirigami.Units.smallSpacing : 0) : 0
    Item {
        id: iconBox
        x: cell.view.compact ? (cell.rtl ? cell.width - width - Kirigami.Units.largeSpacing : Kirigami.Units.largeSpacing) : (parent.width - width) / 2
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
    // The tag dots: at the foot of the picture, or at the end of a compact row.
    TagDots {
        id: dots
        x: cell.view.compact ? (cell.rtl ? Kirigami.Units.largeSpacing : cell.width - width - Kirigami.Units.largeSpacing) : iconBox.x + iconBox.width - width + Math.round(dot * 0.4)
        y: cell.view.compact ? (cell.height - height) / 2 : iconBox.y + iconBox.height - height + Math.round(dot * 0.2)
        colours: cell.tagColours
        dot: cell.view.compact ? Kirigami.Units.gridUnit * 0.7 : Math.max(9, Math.round(cell.view.icon * 0.2))
    }
    // The Git badge: at the top of the picture's corner, or at the end of a compact row.
    GitBadge {
        id: git
        x: cell.view.compact ? (cell.rtl ? Kirigami.Units.largeSpacing + (dots.visible ? dots.width + Kirigami.Units.smallSpacing : 0) : cell.width - width - Kirigami.Units.largeSpacing - (dots.visible ? dots.width + Kirigami.Units.smallSpacing : 0)) : iconBox.x - Math.round(dot * 0.3)
        y: cell.view.compact ? (cell.height - height) / 2 : iconBox.y + iconBox.height - height + Math.round(dot * 0.2)
        code: cell.gitBadge
        dot: cell.view.compact ? Kirigami.Units.gridUnit * 0.8 : Math.max(11, Math.round(cell.view.icon * 0.24))
    }
    Text {
        x: cell.view.compact ? (cell.rtl ? Kirigami.Units.smallSpacing + cell.tail : iconBox.x + iconBox.width + Kirigami.Units.largeSpacing) : Kirigami.Units.smallSpacing
        y: cell.view.compact ? 0 : iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
        width: cell.view.compact ? (cell.rtl ? iconBox.x - Kirigami.Units.largeSpacing - x : cell.width - x - Kirigami.Units.smallSpacing - cell.tail) : cell.width - x - Kirigami.Units.smallSpacing
        height: cell.view.compact ? cell.height : cell.height - y
        verticalAlignment: cell.view.compact ? Text.AlignVCenter : Text.AlignTop
        horizontalAlignment: cell.view.compact ? Text.AlignLeft : Text.AlignHCenter
        visible: !cell.editing
        textFormat: Text.PlainText
        wrapMode: cell.view.compact ? Text.NoWrap : Text.WrapAnywhere
        maximumLineCount: cell.view.compact ? 1 : 2
        elide: Text.ElideRight
        text: cell.name
        color: TelamonStyle.highContrast && cell.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
    }
    Loader {
        x: cell.view.compact ? (cell.rtl ? Kirigami.Units.smallSpacing : iconBox.x + iconBox.width + Kirigami.Units.largeSpacing) : Kirigami.Units.smallSpacing
        y: cell.view.compact ? (cell.height - height) / 2 : iconBox.y + iconBox.height + Kirigami.Units.smallSpacing
        width: cell.view.compact && cell.rtl ? iconBox.x - Kirigami.Units.largeSpacing - x : cell.width - x - Kirigami.Units.smallSpacing
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
