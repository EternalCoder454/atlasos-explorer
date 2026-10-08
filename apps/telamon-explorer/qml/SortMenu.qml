pragma ComponentBehavior: Bound
import QtQuick
import Telamon.Ui

// The Sort menu: what the folder is ordered by. The toolbar's Sort button
// opens it, and the folder's context menu has it as a submenu.
ContextMenu {
    id: menu

    // The window (Main.qml): the tab's view and whether it shows results.
    required property var win

    // See FileMenu: the menu's own keyboard navigation, not the list's.
    Component.onCompleted: contentItem.keyNavigationEnabled = false

    ContextMenuItem {
        text: qsTr("Best Match")
        visible: menu.win.searching
        // A hidden row is off too: the arrow keys would stop on it.
        enabled: menu.win.searching
        radio: true
        checkable: true
        checked: menu.win.view?.folder.sortColumn === FolderModel.Relevance
        onTriggered: menu.win.view.folder.sortColumn = FolderModel.Relevance
    }
    Repeater {
        model: [
            { text: qsTr("Name"), column: FolderModel.Name },
            { text: qsTr("Size"), column: FolderModel.Size },
            { text: qsTr("Type"), column: FolderModel.Type },
            { text: qsTr("Modified"), column: FolderModel.Modified }
        ]
        ContextMenuItem {
            required property var modelData
            text: modelData.text
            radio: true
            checkable: true
            checked: menu.win.view?.folder.sortColumn === modelData.column
            onTriggered: menu.win.view.folder.sortColumn = modelData.column
        }
    }
    ContextMenuSeparator {}
    ContextMenuItem { text: qsTr("Ascending"); enabled: !menu.win.searching || menu.win.view?.folder.sortColumn !== FolderModel.Relevance; radio: true; checkable: true; checked: !menu.win.view?.folder.sortDescending; onTriggered: menu.win.view.folder.sortDescending = false }
    ContextMenuItem { text: qsTr("Descending"); enabled: !menu.win.searching || menu.win.view?.folder.sortColumn !== FolderModel.Relevance; radio: true; checkable: true; checked: menu.win.view?.folder.sortDescending ?? false; onTriggered: menu.win.view.folder.sortDescending = true }
}
