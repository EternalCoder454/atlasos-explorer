pragma ComponentBehavior: Bound
import QtQuick
import Telamon.Ui

// The context menu of one or more items: Open, Open With, an icon row (Cut,
// Copy, Paste, Rename, Trash), Compress when Telamon Archive is installed,
// Properties, and "More Actions". It is made whole before it appears
// (`openFor` asks FileActions for the snapshot, fills the lists, and only then
// shows it) and nothing in it changes while it is open: the rows read the
// snapshot, not the clipboard or the folder. What a row does runs after the
// menu has closed, so a dialog it opens keeps the keyboard.
ContextMenu {
    id: menu

    // The window's FileActions.
    required property var actions
    // What FileActions.itemMenu made for the items under the pointer.
    property var snap: ({})
    property var _command: null

    function has(key) {
        return snap.state !== undefined && snap.state[key] !== undefined;
    }
    function on(key) {
        return snap.state !== undefined && snap.state[key] === true;
    }

    // Shows the menu for `urls` at (x, y) of `anchor`. Nothing is shown for
    // items that are not listed any more.
    function openFor(urls, anchor, x, y) {
        const made = actions.itemMenu(urls);
        if (made.state === undefined) {
            return;
        }
        snap = made;
        openWith.entries = made.openWith;
        more.entries = made.services;
        // After the click that asked for it is over: a popup opened while its
        // own button release is delivered closes with it.
        Qt.callLater(() => menu.popup(anchor, x, y));
    }

    // Runs `fn` once the menu is closed.
    function later(fn) {
        _command = fn;
    }
    // ContextMenu's list also moves its row on Up and Down, besides the
    // menu doing it: it stops on separators (the key seems lost, Enter does
    // nothing there) and, when a row such as IconRowItem holds the keyboard,
    // takes the key so the menu doesn't move on. The menu's own navigation
    // skips separators and hidden rows; that one alone is used here.
    Component.onCompleted: contentItem.keyNavigationEnabled = false
    onClosed: {
        const fn = _command;
        _command = null;
        if (fn) {
            fn();
        }
    }

    function quick(command) {
        const urls = snap.urls;
        switch (command) {
        case "cut":
            actions.copy(urls, true);
            break;
        case "copy":
            actions.copy(urls, false);
            break;
        case "paste":
            actions.paste(snap.pasteIntoFolder ? urls[0] : Qt.url(""));
            break;
        case "rename":
            actions.rename(urls[0]);
            break;
        case "trash":
            actions.trash(urls);
            break;
        }
    }

    ContextMenuItem {
        text: qsTr("Open")
        symbol: Symbols.OpenInNew
        shortcutText: "Enter"
        onTriggered: menu.later(() => menu.actions.openItems(menu.snap.urls))
    }
    ActionMenu {
        id: openWith
        title: qsTr("Open With")
        emptyText: qsTr("No Applications Found")
        onActivated: entry => menu.later(() => menu.actions.runMenuAction(entry.id))
    }
    ContextMenuSeparator {}
    IconRowItem {
        buttons: [
            {
                "symbol": Symbols.ContentCut,
                "text": qsTr("Cut"),
                "enabled": menu.on("cut"),
                "command": "cut"
            },
            {
                "symbol": Symbols.ContentCopy,
                "text": qsTr("Copy"),
                "enabled": menu.on("copy"),
                "command": "copy"
            },
            {
                "symbol": Symbols.ContentPaste,
                "text": menu.snap.pasteIntoFolder === true ? qsTr("Paste Into Folder") : qsTr("Paste"),
                "enabled": menu.on("paste"),
                "command": "paste"
            },
            {
                "symbol": Symbols.DriveFileRenameOutline,
                "text": qsTr("Rename"),
                "enabled": menu.on("rename"),
                "command": "rename"
            },
            {
                "symbol": Symbols.Delete,
                "text": qsTr("Move to Trash"),
                "enabled": menu.on("trash"),
                "destructive": true,
                "command": "trash"
            }
        ]
        onChosen: command => {
            menu.later(() => menu.quick(command));
            menu.dismiss();
        }
    }
    ContextMenuSeparator {}
    ContextMenuItem {
        text: qsTr("Compress…")
        symbol: Symbols.FolderZip
        visible: menu.has("compress")
        // A hidden row is off too: the arrow keys would stop on it.
        enabled: menu.has("compress") && menu.on("compress")
        onTriggered: menu.later(() => menu.actions.compress(menu.snap.urls))
    }
    ContextMenuItem {
        text: qsTr("Properties")
        symbol: Symbols.Info
        onTriggered: menu.later(() => menu.actions.showProperties(menu.snap.urls))
    }
    ContextMenuSeparator {}

    // Rare things, and the service menus, in one place that never moves.
    ActionMenu {
        id: more
        title: qsTr("More Actions")
        ContextMenuItem {
            text: menu.snap.urls && menu.snap.urls.length > 1 ? qsTr("Open in New Tabs") : qsTr("Open in New Tab")
            symbol: Symbols.Tab
            visible: menu.has("openInNewTab")
            enabled: menu.has("openInNewTab")
            onTriggered: menu.later(() => menu.actions.openInNewTabs(menu.snap.urls))
        }
        ContextMenuItem {
            text: qsTr("Open File Location")
            symbol: Symbols.FolderOpen
            shortcutText: "Ctrl+Enter"
            visible: menu.has("openFileLocation")
            enabled: menu.has("openFileLocation")
            onTriggered: menu.later(() => menu.actions.openLocation(menu.snap.urls))
        }
        ContextMenuItem {
            text: qsTr("Open Terminal Here")
            symbol: Symbols.Terminal
            enabled: menu.on("openTerminal")
            onTriggered: menu.later(() => menu.actions.openTerminal(menu.snap.terminalFolder))
        }
        ContextMenuItem {
            text: qsTr("Pin to Sidebar")
            symbol: Symbols.PushPin
            visible: menu.has("pinToSidebar")
            enabled: menu.has("pinToSidebar") && menu.on("pinToSidebar")
            onTriggered: menu.later(() => PlacesLogic.pinFolder(menu.snap.urls[0]))
        }
        ContextMenuSeparator {}
        ContextMenuItem {
            text: qsTr("Copy Path")
            symbol: Symbols.Link
            shortcutText: "Ctrl+Shift+C"
            onTriggered: menu.later(() => menu.actions.copyPath(menu.snap.urls))
        }
        ContextMenuItem {
            text: qsTr("Hide")
            symbol: Symbols.VisibilityOff
            visible: menu.has("hide")
            enabled: menu.has("hide") && menu.on("hide")
            onTriggered: menu.later(() => menu.actions.setHidden(menu.snap.urls, true))
        }
        ContextMenuItem {
            text: qsTr("Unhide")
            symbol: Symbols.Visibility
            visible: menu.has("unhide")
            enabled: menu.has("unhide") && menu.on("unhide")
            onTriggered: menu.later(() => menu.actions.setHidden(menu.snap.urls, false))
        }
        ContextMenuItem {
            text: qsTr("Delete for Good…")
            symbol: Symbols.DeleteForever
            shortcutText: "Shift+Del"
            destructive: true
            enabled: menu.on("deleteForGood")
            onTriggered: menu.later(() => menu.actions.deleteForGood(menu.snap.urls))
        }
        ContextMenuSeparator {
            visible: more.entries.length > 0
        }
        onActivated: entry => menu.later(() => menu.actions.runMenuAction(entry.id))
    }
}
