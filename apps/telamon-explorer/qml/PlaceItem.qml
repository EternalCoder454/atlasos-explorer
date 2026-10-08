pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One place of the sidebar, from PlacesModel's roles: Home, a folder, a pin, a
// drive with its usage bar and eject button, a phone, the Network, the Trash
// with its count. Click opens it (a drive that is not mounted is mounted
// first); Ctrl+click and a middle click open a background tab. A pin can be
// dragged to a new position: the sidebar's drop handler (Main.qml) moves it.
SidebarItem {
    id: item

    required property string placeKey
    required property string placeText
    required property string placeIcon
    required property url placeUrl
    required property int placeKind
    required property bool placeHidden
    required property bool placeMounted
    required property bool placeBusy
    required property int placeUsage
    required property string placeUsageText
    required property string placeValue
    required property string placeTip
    required property bool placeReorder
    required property bool placeCanUnmount

    // The folder shown in the tab, to mark the place that is it.
    property url current

    // The Home and standard folders keep the symbols they always had.
    readonly property var standard: [
        {
            "key": "home",
            "symbol": Symbols.Home
        },
        {
            "key": "desktop",
            "symbol": Symbols.DesktopWindows
        },
        {
            "key": "documents",
            "symbol": Symbols.Description
        },
        {
            "key": "downloads",
            "symbol": Symbols.Download
        },
        {
            "key": "pictures",
            "symbol": Symbols.Image
        },
        {
            "key": "music",
            "symbol": Symbols.MusicNote
        },
        {
            "key": "videos",
            "symbol": Symbols.Movie
        }
    ]
    function symbolOf(): int {
        switch (placeKind) {
        case PlacesLogic.Recent:
            return Symbols.History;
        case PlacesLogic.NetworkPlace:
            return Symbols.Lan;
        case PlacesLogic.Server:
            return Symbols.Dns;
        case PlacesLogic.TrashPlace:
            return Symbols.Delete;
        case PlacesLogic.Drive:
            return Symbols.HardDrive;
        case PlacesLogic.Removable:
            return Symbols.Usb;
        case PlacesLogic.Phone:
            return Symbols.Mobile;
        case PlacesLogic.Folder:
            for (const s of standard) {
                if (PlacesLogic.sameLocation(placeUrl, StandardPlaces.place(s.key))) {
                    return s.symbol;
                }
            }
            return Symbols.Folder;
        }
        return Symbols.Folder;
    }

    readonly property bool hasBar: placeUsage >= 0
    readonly property real barHeight: 4
    readonly property bool ejectable: placeCanUnmount && placeKind === PlacesLogic.Removable

    text: placeText
    symbol: symbolOf()
    value: ejectable ? "" : placeValue
    selected: PlacesLogic.sameLocation(placeUrl, current)
    opacity: placeHidden ? 0.5 : placeBusy ? 0.6 : 1
    // The bar sits under the name.
    implicitHeight: Math.round(Kirigami.Units.gridUnit * 2.1) + (hasBar ? Math.round(barHeight) + 6 : 0)
    bottomPadding: hasBar ? Math.round(barHeight) + 6 : 0
    z: dragHandler.active ? 100 : 0
    Accessible.description: placeUsageText.length > 0 ? placeUsageText : placeValue
    Accessible.name: placeText

    // Ctrl+click opens a new tab, as does a middle click.
    onClicked: PlacesLogic.open(placeKey, TabLogic.controlHeld())
    // The menu: the sidebar asks for it on the Menu key; a right click is
    // taken here (a middle click opens a background tab). A MouseArea sees a
    // press the button itself leaves alone, which a pointer handler on the
    // button or on the scrolling list does not reliably get.
    signal menuRequested(point pos)
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.MiddleButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.MiddleButton) {
                PlacesLogic.open(item.placeKey, true);
            } else {
                item.menuRequested(Qt.point(mouse.x, mouse.y));
            }
        }
    }

    TelamonToolTip {
        text: item.placeTip
        shown: item.placeTip.length > 0 && item.hovered && !item.compact
    }

    // How full the disk is.
    Rectangle {
        visible: item.hasBar && !item.compact
        height: item.barHeight
        radius: height / 2
        anchors {
            left: parent.left
            right: parent.right
            bottom: parent.bottom
            // Under the name: past the icon.
            leftMargin: TelamonStyle.spacingLarge * 2 + Kirigami.Units.iconSizes.smallMedium
            rightMargin: TelamonStyle.spacingLarge
            bottomMargin: 5
        }
        color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
        Accessible.ignored: true
        Rectangle {
            height: parent.height
            radius: parent.radius
            width: Math.max(parent.height, parent.width * Math.min(100, Math.max(0, item.placeUsage)) / 100)
            color: item.placeUsage >= PlacesLogic.nearlyFullPercent ? TelamonStyle.error : TelamonStyle.accent
        }
    }

    // Eject: unmounts a drive that can be taken out.
    T.AbstractButton {
        id: eject
        visible: item.ejectable && !item.compact
        z: 2
        width: Kirigami.Units.gridUnit * 1.7
        height: width
        anchors {
            right: parent.right
            rightMargin: TelamonStyle.spacingSmall
            verticalCenter: parent.verticalCenter
            verticalCenterOffset: item.hasBar ? -Math.round((item.barHeight + 6) / 2) : 0
        }
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        Accessible.name: qsTr("Eject %1").arg(item.placeText)
        onClicked: PlacesLogic.unmount(item.placeKey)
        background: Rectangle {
            radius: TelamonStyle.radiusSmall
            color: eject.down ? TelamonStyle.pressed : eject.hovered ? TelamonStyle.hover : "transparent"
            border.width: eject.visualFocus ? 2 : 0
            border.color: TelamonStyle.focus
        }
        contentItem: Symbol {
            icon: Symbols.Eject
            size: Math.round(Kirigami.Units.iconSizes.small * 1.2)
            color: Kirigami.Theme.textColor
        }
        TelamonToolTip {
            text: qsTr("Eject")
            shown: eject.hovered
        }
    }

    // The drag of a pin to a new position: a copy of the entry follows the
    // pointer, and the sidebar's drop handler moves the place.
    Item {
        id: ghost
        visible: dragHandler.active
        width: item.width
        height: item.height
        property string placeKey: item.placeKey
        Drag.dragType: Drag.Internal
        Drag.supportedActions: Qt.MoveAction
        Drag.hotSpot.x: width / 2
        Drag.hotSpot.y: height / 2
        Rectangle {
            anchors.fill: parent
            radius: TelamonStyle.radiusSmall
            color: TelamonStyle.selection
            opacity: 0.85
            border.width: 1
            border.color: TelamonStyle.accent
            Text {
                anchors.verticalCenter: parent.verticalCenter
                x: TelamonStyle.spacingLarge * 2 + Kirigami.Units.iconSizes.smallMedium
                width: parent.width - x - TelamonStyle.spacingLarge
                text: item.placeText
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
            }
        }
    }
    DragHandler {
        id: dragHandler
        enabled: item.placeReorder
        target: ghost
        xAxis.enabled: false
        grabPermissions: PointerHandler.CanTakeOverFromAnything
        // The drag starts and ends by hand: setting Drag.active false only
        // cancels, a drop is made by drop().
        onActiveChanged: {
            if (active) {
                ghost.Drag.start();
            } else {
                ghost.Drag.drop();
                ghost.x = 0;
                ghost.y = 0;
            }
        }
    }
}
