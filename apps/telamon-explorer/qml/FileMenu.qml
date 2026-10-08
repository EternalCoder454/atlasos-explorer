pragma ComponentBehavior: Bound
import QtQuick
import Telamon.Ui

// The context menu of one or more items: Open, Open With, an icon row (Cut,
// Copy, Paste, Rename, Trash), Extract and Compress when Telamon Archive is installed,
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
    // The tab is split in two panes: Copy to Other Pane and Move to Other Pane
    // show, and these say why they can't be used ("": they can). Set by the window.
    property bool split: false
    property string copyProblem
    property string moveProblem

    // Send these items to the other pane's folder (the window does it).
    signal toOtherPane(var urls, bool move)

    function has(key) {
        return snap.state !== undefined && snap.state[key] !== undefined;
    }
    function on(key) {
        return snap.state !== undefined && snap.state[key] === true;
    }
    // Whether the quick action on pictures `key` (rotateLeft, rotateRight, png,
    // jpeg, webp, combine) applies to the items.
    function pic(key) {
        return snap.pictures !== undefined && snap.pictures[key] === true;
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
        tagRow.colours = made.tags.colours;
        tagsMenu.entries = tagEntries(made.tags);
        // After the click that asked for it is over: a popup opened while its
        // own button release is delivered closes with it.
        Qt.callLater(() => menu.popup(anchor, x, y));
    }

    // The rows under the colour dots of the Tags menu: the named tags in use
    // (checked when every item has it), then New Tag and Clear Tags. When the
    // items can't keep tags, one row says why.
    function tagEntries(info) {
        if (!info.available) {
            return [
                {
                    "text": info.why,
                    "enabled": false
                }
            ];
        }
        const list = [];
        for (const t of info.named) {
            list.push({
                "text": t.state === 1 ? qsTr("%1 (some items)").arg(t.text) : t.text,
                "checkable": true,
                "checked": t.state === 2,
                "kind": "toggle",
                "name": t.name,
                "on": t.state !== 2
            });
        }
        if (list.length > 0) {
            list.push({
                "separator": true
            });
        }
        list.push({
            "text": qsTr("New Tag…"),
            "kind": "new"
        });
        list.push({
            "text": qsTr("Clear Tags"),
            "kind": "clear",
            "enabled": info.hasTags
        });
        return list;
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
            actions.rename(urls);
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
    // Only in the Trash: puts the items back where they were.
    ContextMenuItem {
        text: qsTr("Restore")
        symbol: Symbols.RestoreFromTrash
        visible: menu.has("restore")
        // A hidden row is off too: the arrow keys would stop on it.
        enabled: menu.has("restore") && menu.on("restore")
        onTriggered: menu.later(() => menu.actions.restore(menu.snap.urls))
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
    // Telamon Archive's: items only appear when it is installed (and Extract
    // only for archives); they work on files on this computer.
    ContextMenuItem {
        text: qsTr("Extract Here")
        symbol: Symbols.Unarchive
        visible: menu.has("extractHere")
        enabled: menu.has("extractHere") && menu.on("extractHere")
        onTriggered: menu.later(() => menu.actions.extractHere(menu.snap.urls))
    }
    ContextMenuItem {
        text: qsTr("Extract To…")
        symbol: Symbols.Unarchive
        visible: menu.has("extractTo")
        enabled: menu.has("extractTo") && menu.on("extractTo")
        onTriggered: menu.later(() => menu.actions.extractTo(menu.snap.urls))
    }
    ContextMenuItem {
        text: qsTr("Compress to ZIP")
        symbol: Symbols.FolderZip
        visible: menu.has("compressZip")
        enabled: menu.has("compressZip") && menu.on("compressZip")
        onTriggered: menu.later(() => menu.actions.compressToZip(menu.snap.urls))
    }
    ContextMenuItem {
        text: qsTr("Compress…")
        symbol: Symbols.FolderZip
        visible: menu.has("compress")
        // A hidden row is off too: the arrow keys would stop on it.
        enabled: menu.has("compress") && menu.on("compress")
        onTriggered: menu.later(() => menu.actions.compress(menu.snap.urls))
    }
    ActionMenu {
        id: tagsMenu
        title: qsTr("Tags")
        visible: menu.has("tags")
        // A hidden row is off too: the arrow keys would stop on it.
        enabled: menu.has("tags") && menu.on("tags")
        TagRowItem {
            id: tagRow
            visible: menu.snap.tags !== undefined && menu.snap.tags.available === true
            onChosen: (name, on) => {
                menu.later(() => menu.actions.toggleTag(menu.snap.urls, name, on));
                menu.dismiss();
            }
        }
        ContextMenuSeparator {
            visible: tagRow.visible
        }
        onActivated: entry => {
            if (entry.kind === "toggle") {
                menu.later(() => menu.actions.toggleTag(menu.snap.urls, entry.name, entry.on));
            } else if (entry.kind === "new") {
                menu.later(() => menu.actions.newTag(menu.snap.urls));
            } else if (entry.kind === "clear") {
                menu.later(() => menu.actions.clearTags(menu.snap.urls));
            }
        }
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
        ContextMenuSeparator {
            visible: menu.split
        }
        ContextMenuItem {
            text: qsTr("Copy to Other Pane")
            symbol: Symbols.FileCopy
            shortcutText: "F5"
            visible: menu.split
            // A hidden row is off too: the arrow keys would stop on it.
            enabled: menu.split && menu.copyProblem.length === 0
            onTriggered: menu.later(() => menu.toOtherPane(menu.snap.urls, false))
        }
        ContextMenuItem {
            text: qsTr("Move to Other Pane")
            symbol: Symbols.DriveFileMove
            shortcutText: "F6"
            visible: menu.split
            enabled: menu.split && menu.moveProblem.length === 0
            onTriggered: menu.later(() => menu.toOtherPane(menu.snap.urls, true))
        }
        // Quick actions on pictures, and PDFs to join: new files, the originals stay.
        ContextMenuSeparator {
            visible: menu.snap.pictures !== undefined && menu.snap.pictures.any === true
        }
        ContextMenuItem {
            text: qsTr("Rotate Left")
            symbol: Symbols.Rotate90DegreesCcw
            visible: menu.pic("rotateLeft")
            enabled: menu.pic("rotateLeft")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 0))
        }
        ContextMenuItem {
            text: qsTr("Rotate Right")
            symbol: Symbols.Rotate90DegreesCw
            visible: menu.pic("rotateRight")
            enabled: menu.pic("rotateRight")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 1))
        }
        ContextMenuItem {
            text: qsTr("Convert to PNG")
            symbol: Symbols.Transform
            visible: menu.pic("png")
            enabled: menu.pic("png")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 2))
        }
        ContextMenuItem {
            text: qsTr("Convert to JPEG")
            symbol: Symbols.Transform
            visible: menu.pic("jpeg")
            enabled: menu.pic("jpeg")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 3))
        }
        ContextMenuItem {
            text: qsTr("Convert to WebP")
            symbol: Symbols.Transform
            visible: menu.pic("webp")
            enabled: menu.pic("webp")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 4))
        }
        ContextMenuItem {
            text: qsTr("Combine into PDF")
            symbol: Symbols.PictureAsPdf
            visible: menu.pic("combine")
            enabled: menu.pic("combine")
            onTriggered: menu.later(() => menu.actions.pictureAction(menu.snap.urls, 5))
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
