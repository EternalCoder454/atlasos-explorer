pragma ComponentBehavior: Bound
import QtQuick
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One saved search of the sidebar's Saved Searches section: its name, and a
// tooltip that says what it does (the words, the scope, the filters). Click
// runs it again in the tab shown; a right click or the Menu key opens its
// menu (Rename, Remove). Nothing but the search is kept: no results.
SidebarItem {
    id: item

    // {id, name, tip, scope, folder, query, ...} from SavedLogic.items.
    required property var modelData
    readonly property var saved: modelData
    readonly property int savedId: saved.id

    signal chosen(int id)
    signal menuRequested(point pos)

    text: saved.name
    symbol: Symbols.SavedSearch
    Accessible.name: saved.name
    Accessible.description: saved.tip
    onClicked: item.chosen(saved.id)

    // A MouseArea sees a right click the button itself leaves alone, which a
    // pointer handler on the button or on the scrolling list does not reliably get.
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: mouse => item.menuRequested(Qt.point(mouse.x, mouse.y))
    }

    TelamonToolTip {
        text: item.saved.tip
        shown: item.saved.tip.length > 0 && item.hovered && !item.compact
    }
}
