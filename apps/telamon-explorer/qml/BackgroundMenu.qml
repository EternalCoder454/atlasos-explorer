pragma ComponentBehavior: Bound
import QtQuick
import Telamon.Ui

// The context menu of the folder's empty space: New (Folder, Text File and the
// templates of ~/Templates), Paste, Undo and Redo by name, Sort and View,
// Open Terminal Here. Made whole before it appears and not changed while it is
// open, like FileMenu; what a row does runs after the menu has closed.
ContextMenu {
    id: menu

    // The window's FileActions, and the window (for the Sort and View menus).
    required property var actions
    required property var win
    property var snap: ({})
    property var _command: null

    // See FileMenu: the menu's own keyboard navigation, not the list's.
    Component.onCompleted: contentItem.keyNavigationEnabled = false

    function has(key) {
        return snap.state !== undefined && snap.state[key] !== undefined;
    }
    function on(key) {
        return snap.state !== undefined && snap.state[key] === true;
    }

    function openFor(anchor, x, y) {
        const made = actions.backgroundMenu();
        if (made.state === undefined) {
            return;
        }
        snap = made;
        const writable = made.state.new === true;
        newMenu.entries = made.templates.map(t => ({
                    "text": t.label,
                    "icon": "text-x-generic",
                    "enabled": writable,
                    "file": t.file
                }));
        Qt.callLater(() => menu.popup(anchor, x, y));
    }

    function later(fn) {
        _command = fn;
    }
    onClosed: {
        const fn = _command;
        _command = null;
        if (fn) {
            fn();
        }
    }

    ActionMenu {
        id: newMenu
        title: qsTr("New")
        ContextMenuItem {
            text: qsTr("Folder")
            symbol: Symbols.CreateNewFolder
            shortcutText: "Ctrl+Shift+N"
            enabled: menu.on("new")
            onTriggered: menu.later(() => menu.actions.newFolder())
        }
        ContextMenuItem {
            text: qsTr("Text File")
            symbol: Symbols.NoteAdd
            enabled: menu.on("new")
            onTriggered: menu.later(() => menu.actions.newFile(Qt.url("")))
        }
        ContextMenuSeparator {
            visible: newMenu.entries.length > 0
        }
        onActivated: entry => menu.later(() => menu.actions.newFile(entry.file))
    }
    ContextMenuItem {
        text: qsTr("Paste")
        symbol: Symbols.ContentPaste
        shortcutText: "Ctrl+V"
        enabled: menu.on("paste")
        onTriggered: menu.later(() => menu.actions.paste(Qt.url("")))
    }
    ContextMenuSeparator {}
    ContextMenuItem {
        text: menu.snap.undoText ?? qsTr("Undo")
        symbol: Symbols.Undo
        shortcutText: "Ctrl+Z"
        enabled: menu.on("undo")
        onTriggered: menu.later(() => menu.actions.undo())
    }
    ContextMenuItem {
        text: menu.snap.redoText ?? qsTr("Redo")
        symbol: Symbols.Redo
        shortcutText: "Ctrl+Shift+Z"
        enabled: menu.on("redo")
        onTriggered: menu.later(() => menu.actions.redo())
    }
    ContextMenuSeparator {}
    SortMenu {
        title: qsTr("Sort")
        win: menu.win
    }
    ViewMenu {
        title: qsTr("View")
        win: menu.win
    }
    ContextMenuSeparator {}
    ContextMenuItem {
        text: qsTr("Open Terminal Here")
        symbol: Symbols.Terminal
        shortcutText: "Shift+F4"
        enabled: menu.on("openTerminal")
        onTriggered: menu.later(() => menu.actions.openTerminal(menu.snap.terminalFolder))
    }
    ContextMenuItem {
        text: qsTr("Pin This Folder to Sidebar")
        symbol: Symbols.PushPin
        visible: menu.has("pinFolder")
        enabled: menu.has("pinFolder") && menu.on("pinFolder")
        onTriggered: menu.later(() => PlacesLogic.pinFolder(menu.snap.folder))
    }
    ContextMenuSeparator {}
    ContextMenuItem {
        text: qsTr("Properties")
        symbol: Symbols.Info
        visible: menu.has("properties")
        enabled: menu.has("properties") && menu.on("properties")
        onTriggered: menu.later(() => menu.actions.showProperties([]))
    }
}
