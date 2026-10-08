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
    // The keyboard goes back to the folder, so the arrow keys work on what was just chosen.
    onClosed: {
        if (menu.win.view) {
            Qt.callLater(() => menu.win.view.forceActiveFocus());
        }
    }

    ContextMenuItem { text: qsTr("Details"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "details"; onTriggered: menu.win.view.viewMode = "details" }
    ContextMenuItem { text: qsTr("Icons"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "icons"; onTriggered: menu.win.view.viewMode = "icons" }
    ContextMenuItem { text: qsTr("Compact"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "compact"; onTriggered: menu.win.view.viewMode = "compact" }
    ContextMenuItem { text: qsTr("Columns"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "columns"; onTriggered: menu.win.view.viewMode = "columns" }
    ContextMenuItem { text: qsTr("Gallery"); radio: true; checkable: true; checked: menu.win.view?.viewMode === "gallery"; onTriggered: menu.win.view.viewMode = "gallery" }
    ContextMenuSeparator {}
    ContextMenuItem { text: qsTr("Show Hidden Files"); shortcutText: "Ctrl+H"; checkable: true; checked: menu.win.view?.folder.showHidden ?? false; onTriggered: menu.win.toggleHidden() }
    ContextMenuItem { text: qsTr("Preview Pane"); shortcutText: "Alt+P"; checkable: true; checked: PreviewLogic.paneShown; onTriggered: PreviewLogic.paneShown = !PreviewLogic.paneShown }
    // Thumbnails and previews of files on a server download them, so they are off until asked for.
    ContextMenuItem { text: qsTr("Preview Files on Servers"); checkable: true; checked: ServerLogic.previewRemote; onTriggered: menu.win.setPreviewRemote(!ServerLogic.previewRemote) }
    ContextMenuSeparator {}
    ContextMenuItem { text: qsTr("Zoom In"); shortcutText: "Ctrl++"; onTriggered: menu.win.zoom(1) }
    ContextMenuItem { text: qsTr("Zoom Out"); shortcutText: "Ctrl+-"; onTriggered: menu.win.zoom(-1) }
    ContextMenuItem { text: qsTr("Reset Zoom"); shortcutText: "Ctrl+0"; onTriggered: menu.win.zoom(0) }
    ContextMenuSeparator {}
    // Each folder remembers how it is shown (in Files' settings, never in the folder).
    ContextMenuItem { text: qsTr("Use the Same View for Every Folder"); checkable: true; checked: ViewMemory.sameForAll; onTriggered: menu.win.setSameView(!ViewMemory.sameForAll) }
    ContextMenuItem {
        text: qsTr("Reset This Folder's View")
        enabled: !ViewMemory.sameForAll && menu.win.view !== null && menu.win.hasRememberedView(ViewMemory.revision)
        onTriggered: menu.win.view.resetRemembered()
    }
}
