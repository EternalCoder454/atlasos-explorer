pragma ComponentBehavior: Bound
import QtQuick
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One tag of the sidebar's Tags section: a coloured dot for a colour tag, a
// label symbol for a named one, the name, and how many items have it when the
// index knows. Click lists every item with the tag (the tab's search).
SidebarItem {
    id: item

    // {name, text, colour, count} from TagLogic.sidebarTags.
    required property var modelData
    readonly property var tag: modelData
    // The tag the tab is listing now (empty: none), to mark its row.
    property string current

    signal chosen(string name)

    text: tag.text
    value: tag.count > 0 ? String(tag.count) : ""
    // A colour tag draws its own dot in the icon's place.
    symbol: tag.colour.length > 0 ? 0 : Symbols.Sell
    selected: current.length > 0 && current.toLowerCase() === tag.name.toLowerCase()
    Accessible.name: tag.text
    Accessible.description: tag.count > 0 ? (tag.count === 1 ? qsTr("1 item") : qsTr("%1 items").arg(tag.count)) : ""
    onClicked: item.chosen(tag.name)

    Rectangle {
        visible: item.tag.colour.length > 0
        x: TelamonStyle.spacingLarge + (Kirigami.Units.iconSizes.smallMedium - width) / 2
        y: (item.height - height) / 2
        width: Math.round(Kirigami.Units.iconSizes.smallMedium * 0.7)
        height: width
        radius: width / 2
        color: item.tag.colour
    }
}
