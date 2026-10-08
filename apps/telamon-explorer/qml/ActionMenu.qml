pragma ComponentBehavior: Bound
import QtQuick
import Telamon.Ui

// A ContextMenu whose last rows are made from a list of entries, all at once
// when `entries` is set (so before the menu is shown, never while it is open):
// the "Open With" list, a service menu's actions, the templates of "New".
// Rows declared in the menu itself come first (the made rows are added after them). An entry is {text, icon,
// enabled, checkable, checked, separator, children: [entries]}; one row is
// chosen with `activated(entry)`, whatever submenu it is in. The text is the
// entry's own and is shown as plain text.
// The rows are made from components, so qmllint sees them as plain objects.
// qmllint disable missing-property
ContextMenu {
    id: menu

    property var entries: []
    // Shown, disabled, when there are no entries.
    property string emptyText
    signal activated(var entry)

    // The rows made here, to take them out when the entries change.
    property var _made: []
    Component.onCompleted: {
        // See FileMenu: the menu's own keyboard navigation, not the list's.
        contentItem.keyNavigationEnabled = false;
        rebuild();
    }
    onEntriesChanged: rebuild()

    Component {
        id: itemComponent
        ContextMenuItem {}
    }
    Component {
        id: separatorComponent
        ContextMenuSeparator {}
    }
    Component {
        id: groupComponent
        ContextMenu {
            Component.onCompleted: contentItem.keyNavigationEnabled = false
        }
    }

    function _clear() {
        for (const m of _made) {
            if (m.menu) {
                removeMenu(m.object);
            } else {
                removeItem(m.object);
            }
        }
        _made = [];
    }

    function _item(entry, into, made) {
        // An item needs an item as its parent while it is made, and it stays
        // there until its menu is shown: its own menu's list.
        const it = itemComponent.createObject(into.contentItem, {
            "text": entry.text ?? "",
            "enabled": entry.enabled !== false,
            "checkable": entry.checkable === true,
            "checked": entry.checked === true
        });
        if (entry.icon) {
            it.icon.name = entry.icon;
        }
        it.triggered.connect(() => menu.activated(entry));
        into.addItem(it);
        if (made) {
            made.push({
                "object": it,
                "menu": false
            });
        }
    }

    function _fill(list, into, made) {
        for (const e of list) {
            if (e.separator === true) {
                const s = separatorComponent.createObject(into.contentItem);
                into.addItem(s);
                if (made) {
                    made.push({
                        "object": s,
                        "menu": false
                    });
                }
            } else if (e.children && e.children.length > 0) {
                const g = groupComponent.createObject(menu, {
                    "title": e.text ?? "",
                    "enabled": e.enabled !== false
                });
                if (e.icon) {
                    g.icon.name = e.icon;
                }
                // Two levels at most: the children are rows.
                for (const c of e.children) {
                    if (c.separator === true) {
                        g.addItem(separatorComponent.createObject(g.contentItem));
                    } else {
                        _item(c, g, null);
                    }
                }
                into.addMenu(g);
                if (made) {
                    made.push({
                        "object": g,
                        "menu": true
                    });
                }
            } else {
                _item(e, into, made);
            }
        }
    }

    function rebuild() {
        _clear();
        const made = [];
        if (entries.length === 0 && emptyText.length > 0) {
            _item({
                "text": emptyText,
                "enabled": false
            }, menu, made);
        } else {
            _fill(entries, menu, made);
        }
        _made = made;
    }
}
