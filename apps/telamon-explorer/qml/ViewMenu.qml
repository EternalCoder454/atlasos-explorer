import QtQuick
import Telamon.Ui

// The View menu: how the folder is shown. The toolbar's View button opens it,
// and the folder's context menu has it as a submenu.
ContextMenu {
    id: menu

    // The window (Main.qml): the tab's view and the zoom.
    required property var win

    // See FileMenu: the menu's own keyboard navigation, not the list's.
    Component.onCompleted: contentItem.keyNavigationEnabled = false

    ContextMenuItem { text: qsTr("Details"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "details"; onTriggered: menu.win.view.viewMode = "details" }
    ContextMenuItem { text: qsTr("Icons"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "icons"; onTriggered: menu.win.view.viewMode = "icons" }
    ContextMenuItem { text: qsTr("Compact"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "compact"; onTriggered: menu.win.view.viewMode = "compact" }
    ContextMenuSeparator {}
    ContextMenuItem { text: qsTr("Show Hidden Files"); shortcutText: "Ctrl+H"; checkable: true; checked: menu.win.view?.folder.showHidden ?? false; onTriggered: menu.win.toggleHidden() }
    ContextMenuItem { text: qsTr("Preview Pane"); shortcutText: "Alt+P"; checkable: true; checked: PreviewLogic.paneShown; onTriggered: PreviewLogic.paneShown = !PreviewLogic.paneShown }
    ContextMenuSeparator {}
    ContextMenuItem { text: qsTr("Zoom In"); shortcutText: "Ctrl++"; onTriggered: menu.win.zoom(1) }
    ContextMenuItem { text: qsTr("Zoom Out"); shortcutText: "Ctrl+-"; onTriggered: menu.win.zoom(-1) }
    ContextMenuItem { text: qsTr("Reset Zoom"); shortcutText: "Ctrl+0"; onTriggered: menu.win.zoom(0) }
}
