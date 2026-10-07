import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Explorer's window: the sidebar's places, the tab strip, the location, the
// command bar and, below, the page of the tab shown (FilesTabPage). The
// breadcrumb, search, panes and the status line come later (docs/ROADMAP.md).
TelamonWindow {
    id: root

    // The Rust backend (src/backend.rs); main.cpp sets it.
    required property var backend

    // The tabs: one FilesTabPage each (kept alive while hidden, so a tab
    // keeps its history, view, selection and scroll). `pages` maps a tab's id
    // to its page; the model feeds the tab bar.
    ListModel {
        id: tabsModel
    }
    property var pages: ({})
    property int nextTabId: 1
    property int currentIndex: -1
    property FilesTabPage page: null
    readonly property FolderView view: page ? page.view : null
    // Closed tabs, the newest last (at most TabLogic.maxClosed).
    property var closedTabs: []
    // Tabs opened in a row from one tab keep their click order beside it.
    property int openerId: -1
    property int openerRun: 0
    // Nothing was done in the window since it started: a launch that asks for
    // a folder shows it in the first tab instead of opening a second one.
    property bool freshStart: true
    property bool restoreTabs: false

    // The folder shown (FolderView.url follows redirects and removed folders).
    readonly property url currentUrl: page ? page.location : Qt.url("")
    // The place whose URL is the folder shown, if any.
    readonly property string place: {
        const cur = currentUrl.toString();
        for (const p of places) {
            if (StandardPlaces.place(p.key).toString() === cur) {
                return p.key;
            }
        }
        return "";
    }
    // What the last launch asked for that Explorer refused, shown over the view.
    property string launchText
    property bool editingAddress: false
    readonly property var selected: view ? view.selectedUrls : []
    readonly property bool hasSelection: selected.length > 0
    readonly property bool canWrite: view ? view.folder.canWrite : false

    FileActions {
        id: fileActions
        folder: root.view ? root.view.folder : null
        window: root
        onFailed: text => root.launchText = text
        onNavigateRequested: target => root.navigate(target)
        onOpenInNewTabRequested: folders => {
            for (const f of folders) {
                root.openInNewTab(f);
            }
        }
    }

    Component {
        id: pageComponent
        FilesTabPage {
            actions: fileActions
        }
    }

    // ---- Address ----
    function startAddressEdit() {
        if (!page) {
            return;
        }
        const u = currentUrl;
        address.text = u.toString().startsWith("file:") ? decodeURIComponent(u.toString().substring(7)) : u.toString();
        editingAddress = true;
        address.forceActiveFocus();
        address.selectAll();
    }
    function endAddressEdit() {
        editingAddress = false;
        if (view) {
            view.forceActiveFocus();
        }
    }
    function goToAddress() {
        const r = fileActions.parseAddress(address.text);
        if (r.ok) {
            launchText = "";
            navigate(Qt.url(r.text));
            endAddressEdit();
        } else {
            launchText = r.text;
        }
    }
    function toggleHidden() {
        if (!view) {
            return;
        }
        view.folder.showHidden = !view.folder.showHidden;
        fileActions.saveShowHidden(view.folder.showHidden);
    }

    // ---- Navigation in the tab shown ----
    function navigate(target) {
        if (page) {
            page.navigate(target);
        }
    }
    function goBack() {
        if (page) {
            page.goBack();
        }
    }
    function goForward() {
        if (page) {
            page.goForward();
        }
    }

    // ---- Tabs ----
    function indexOfTab(id) {
        for (let i = 0; i < tabsModel.count; ++i) {
            if (tabsModel.get(i).tabId === id) {
                return i;
            }
        }
        return -1;
    }
    function pageAt(i) {
        return i >= 0 && i < tabsModel.count ? pages[tabsModel.get(i).tabId] : null;
    }
    // Keeps a tab's title and tooltip in step with its folder.
    function syncTab(p) {
        const i = indexOfTab(p.tabId);
        if (i >= 0) {
            tabsModel.setProperty(i, "title", p.title);
            tabsModel.setProperty(i, "toolTip", p.toolTip);
        }
        markSession();
    }

    // Shows the tab at `i`.
    function selectTab(i) {
        const p = pageAt(i);
        if (!p) {
            return;
        }
        if (page && page !== p) {
            page.visible = false;
        }
        currentIndex = i;
        page = p;
        p.visible = true;
        tabsModel.setProperty(i, "modified", false);
        openerId = -1;
        editingAddress = false;
        p.view.forceActiveFocus();
        markSession();
    }

    // Opens a tab for `target`. Options: `background` (stay on the current
    // tab; the new one is marked until it is shown), `opener` (put it beside
    // the tab it was opened from), `index`, `viewMode`, `back`, `forward`,
    // `select` (URLs to select once listed), `lazy` (list when first shown).
    // Returns the page, or null when the window has as many tabs as it allows.
    function addTab(target, options) {
        const o = options || {};
        if (tabsModel.count >= TabLogic.maxTabs) {
            launchText = qsTr("This window has as many tabs as it can hold (%1). Close one to open another.").arg(TabLogic.maxTabs);
            return null;
        }
        const id = nextTabId++;
        const p = pageComponent.createObject(pageHost, {
            "tabId": id,
            "startUrl": target,
            "startViewMode": o.viewMode || "details",
            "startBack": o.back || [],
            "startForward": o.forward || [],
            "lazy": !!o.lazy,
            "pendingSelect": [],
            "visible": false
        });
        if (!p) {
            return null;
        }
        pages[id] = p;
        p.openInNewTab.connect(u => root.openInNewTab(u));
        p.navigated.connect(() => root.freshStart = false);
        p.titleChanged.connect(() => root.syncTab(p));
        p.toolTipChanged.connect(() => root.syncTab(p));
        p.locationChanged.connect(() => root.markSession());

        let at = tabsModel.count;
        if (o.index !== undefined) {
            at = TabLogic.reopenIndex(tabsModel.count, o.index);
        } else if (o.opener) {
            const run = openerId === o.opener ? openerRun : 0;
            at = TabLogic.insertAfterOpener(tabsModel.count, currentIndex, run);
            openerId = o.opener;
            openerRun = run + 1;
        }
        tabsModel.insert(at, {
            "tabId": id,
            "title": p.title,
            "toolTip": p.toolTip,
            "modified": !!o.background
        });
        if (at <= currentIndex) {
            currentIndex++;
        }
        if (o.select && o.select.length > 0) {
            p.showItems(o.select);
        }
        if (!o.background) {
            selectTab(at);
        } else {
            markSession();
        }
        return p;
    }

    // A folder in a new tab beside the current one, in the background.
    function openInNewTab(target) {
        freshStart = false;
        addTab(target, {
            "background": true,
            "opener": page ? page.tabId : 0
        });
    }
    function newTab() {
        freshStart = false;
        addTab(StandardPlaces.place("home"), {});
    }
    function duplicateTab(i) {
        const p = pageAt(i);
        if (p) {
            freshStart = false;
            addTab(p.location, {
                "index": i + 1,
                "viewMode": p.view.viewMode,
                "back": p.backStack.slice(-TabLogic.maxHistory),
                "forward": p.forwardStack.slice(-TabLogic.maxHistory)
            });
        }
    }

    function closeTab(i) {
        const p = pageAt(i);
        if (!p) {
            return;
        }
        const next = TabLogic.afterClose(tabsModel.count, currentIndex, i);
        if (next < 0) {
            // The last tab: the window closes (and keeps this tab for the next start).
            saveSessionNow();
            root.close();
            return;
        }
        freshStart = false;
        const kept = {
            "url": p.location,
            "viewMode": p.view.viewMode,
            "back": p.backStack.slice(-TabLogic.maxHistory),
            "forward": p.forwardStack.slice(-TabLogic.maxHistory),
            "index": i
        };
        closedTabs = closedTabs.concat([kept]).slice(-TabLogic.maxClosed);
        if (p === page) {
            page = null;
        }
        tabsModel.remove(i);
        delete pages[p.tabId];
        p.visible = false;
        p.destroy();
        selectTab(next);
    }
    function closeOthers(i) {
        for (let n = tabsModel.count - 1; n >= 0; --n) {
            if (n !== i) {
                closeTab(n);
                if (n < i) {
                    i--;
                }
            }
        }
        selectTab(i);
    }
    function closeToTheRight(i) {
        for (let n = tabsModel.count - 1; n > i; --n) {
            closeTab(n);
        }
    }
    function reopenClosedTab() {
        if (closedTabs.length === 0) {
            return;
        }
        const t = closedTabs[closedTabs.length - 1];
        closedTabs = closedTabs.slice(0, -1);
        freshStart = false;
        addTab(t.url, {
            "index": t.index,
            "viewMode": t.viewMode,
            "back": t.back,
            "forward": t.forward
        });
    }
    function moveTab(from, to) {
        if (from === to || from < 0 || to < 0 || from >= tabsModel.count || to >= tabsModel.count) {
            return;
        }
        const now = TabLogic.afterMove(tabsModel.count, currentIndex, from, to);
        tabsModel.move(from, to, 1);
        currentIndex = now;
        markSession();
    }
    function cycleTab(step) {
        selectTab(TabLogic.cycle(tabsModel.count, currentIndex, step));
    }
    function jumpToTab(n) {
        const i = TabLogic.jump(tabsModel.count, n);
        if (i >= 0) {
            selectTab(i);
        }
    }

    // ---- The tabs kept for the next start ----
    function setRestoreTabs(on) {
        restoreTabs = on;
        TabLogic.setRestoreOnStart(on);
        saveSessionNow();
    }
    function saveSessionNow() {
        if (!restoreTabs || tabsModel.count === 0) {
            return;
        }
        const urls = [];
        for (let i = 0; i < tabsModel.count; ++i) {
            urls.push(TabLogic.encode(pageAt(i).location));
        }
        TabLogic.saveSession(urls, currentIndex);
    }
    function markSession() {
        if (restoreTabs) {
            saveTimer.restart();
        }
    }
    Timer {
        id: saveTimer
        interval: 500
        onTriggered: root.saveSessionNow()
    }
    onClosing: root.saveSessionNow()

    // ---- Launches ----
    // Opens what a launch or FileManager1 asked for in tabs of this window: a
    // folder as it is, a file (or any location with `select`) as its folder
    // with it selected. The first one is shown; the others open behind it.
    function openLocations(locations, select) {
        const wanted = [];
        for (const l of locations) {
            const u = Qt.url(l);
            if (select || StandardPlaces.isLocalFile(u)) {
                const folder = StandardPlaces.parentUrl(u);
                const known = wanted.find(w => w.items.length > 0 && w.folder.toString() === folder.toString());
                if (known) {
                    known.items.push(u);
                } else {
                    wanted.push({
                        "folder": folder,
                        "items": [u]
                    });
                }
            } else {
                wanted.push({
                    "folder": u,
                    "items": []
                });
            }
        }
        const first = wanted.length > 0 ? wanted[0] : null;
        for (const w of wanted) {
            if (w === first && freshStart && tabsModel.count === 1) {
                page.showHere(w.folder);
                if (w.items.length > 0) {
                    page.showItems(w.items);
                }
            } else {
                addTab(w.folder, {
                    "background": w !== first,
                    "select": w.items
                });
            }
        }
        freshStart = false;
    }

    title: TelamonApp.name
    width: Kirigami.Units.gridUnit * 64
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 24
    minimumHeight: Kirigami.Units.gridUnit * 18
    stateKey: "main"
    visible: true
    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true

    readonly property var places: [
        { key: "home", text: qsTr("Home"), symbol: Symbols.Home },
        { key: "recent", text: qsTr("Recent"), symbol: Symbols.History },
        { key: "desktop", text: qsTr("Desktop"), symbol: Symbols.DesktopWindows },
        { key: "documents", text: qsTr("Documents"), symbol: Symbols.Description },
        { key: "downloads", text: qsTr("Downloads"), symbol: Symbols.Download },
        { key: "pictures", text: qsTr("Pictures"), symbol: Symbols.Image },
        { key: "music", text: qsTr("Music"), symbol: Symbols.MusicNote },
        { key: "videos", text: qsTr("Videos"), symbol: Symbols.Movie },
        { key: "network", text: qsTr("Network"), symbol: Symbols.Lan },
        { key: "trash", text: qsTr("Trash"), symbol: Symbols.Delete }
    ]

    Connections {
        target: root.backend
        function onOpen(locations, select, newWindow, split) {
            // A launch with no location only raises the window. A new window
            // and split view come later: the locations open as tabs.
            if (locations.length > 0) {
                root.launchText = "";
                root.openLocations(locations, select);
            }
        }
        function onInspected(locations) {
            fileActions.showProperties(locations.map(l => Qt.url(l)));
        }
        function onRefused(text) {
            root.launchText = qsTr("Could not open:") + "\n" + text;
        }
    }

    Component.onCompleted: {
        restoreTabs = TabLogic.restoreOnStart();
        const saved = restoreTabs ? TabLogic.savedSession() : null;
        if (saved && saved.urls.length > 0) {
            // Only the tab shown lists its folder now; the others when first shown.
            for (let i = 0; i < saved.urls.length; ++i) {
                addTab(Qt.url(saved.urls[i]), {
                    "background": true,
                    "lazy": i !== saved.current
                });
                tabsModel.setProperty(i, "modified", false);
            }
            selectTab(saved.current);
            freshStart = false;
        } else {
            addTab(StandardPlaces.place("home"), {});
        }
    }

    // The keys. Those that mean something to a text field are off while the
    // address is being typed.
    Shortcut { sequence: "Ctrl+C"; enabled: !root.editingAddress && root.hasSelection; onActivated: fileActions.copy(root.selected, false) }
    Shortcut { sequence: "Ctrl+X"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: fileActions.copy(root.selected, true) }
    Shortcut { sequence: "Ctrl+V"; enabled: !root.editingAddress && root.canWrite; onActivated: fileActions.paste() }
    Shortcut { sequence: "Ctrl+Z"; enabled: !root.editingAddress; onActivated: fileActions.undo() }
    Shortcut { sequence: "Delete"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: fileActions.trash(root.selected) }
    Shortcut { sequence: "Shift+Delete"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: fileActions.deleteForGood(root.selected) }
    Shortcut { sequence: "Ctrl+Shift+N"; enabled: root.canWrite; onActivated: fileActions.newFolder() }
    Shortcut { sequence: "Ctrl+H"; enabled: !root.editingAddress; onActivated: root.toggleHidden() }
    Shortcut { sequence: "Shift+F4"; onActivated: fileActions.openTerminal() }
    Shortcut { sequence: "Ctrl+L"; onActivated: root.startAddressEdit() }
    Shortcut { sequence: "F6"; onActivated: root.startAddressEdit() }
    Shortcut { sequence: "Alt+D"; onActivated: root.startAddressEdit() }
    Shortcut { sequence: "Alt+Left"; enabled: !root.editingAddress; onActivated: root.goBack() }
    Shortcut { sequence: "Alt+Right"; enabled: !root.editingAddress; onActivated: root.goForward() }
    Shortcut { sequence: "Ctrl+T"; onActivated: root.newTab() }
    Shortcut { sequence: "Ctrl+W"; onActivated: root.closeTab(root.currentIndex) }
    Shortcut { sequence: "Ctrl+Shift+T"; onActivated: root.reopenClosedTab() }
    Shortcut { sequences: ["Ctrl+Tab", "Ctrl+PgDown"]; onActivated: root.cycleTab(1) }
    Shortcut { sequences: ["Ctrl+Shift+Tab", "Ctrl+Shift+Backtab", "Ctrl+PgUp"]; onActivated: root.cycleTab(-1) }
    Shortcut { sequence: "Alt+1"; onActivated: root.jumpToTab(1) }
    Shortcut { sequence: "Alt+2"; onActivated: root.jumpToTab(2) }
    Shortcut { sequence: "Alt+3"; onActivated: root.jumpToTab(3) }
    Shortcut { sequence: "Alt+4"; onActivated: root.jumpToTab(4) }
    Shortcut { sequence: "Alt+5"; onActivated: root.jumpToTab(5) }
    Shortcut { sequence: "Alt+6"; onActivated: root.jumpToTab(6) }
    Shortcut { sequence: "Alt+7"; onActivated: root.jumpToTab(7) }
    Shortcut { sequence: "Alt+8"; onActivated: root.jumpToTab(8) }
    Shortcut { sequence: "Alt+9"; onActivated: root.jumpToTab(9) }

    ContextMenu {
        id: viewMenu
        ContextMenuItem { text: qsTr("Details"); radio: true; checkable: true; checked: root.view?.viewMode === "details"; onTriggered: root.view.viewMode = "details" }
        ContextMenuItem { text: qsTr("Icons"); radio: true; checkable: true; checked: root.view?.viewMode === "icons"; onTriggered: root.view.viewMode = "icons" }
        ContextMenuItem { text: qsTr("Compact"); radio: true; checkable: true; checked: root.view?.viewMode === "compact"; onTriggered: root.view.viewMode = "compact" }
    }

    ContextMenu {
        id: sortMenu
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
                checked: root.view?.folder.sortColumn === modelData.column
                onTriggered: root.view.folder.sortColumn = modelData.column
            }
        }
        ContextMenuSeparator {}
        ContextMenuItem { text: qsTr("Ascending"); radio: true; checkable: true; checked: !root.view?.folder.sortDescending; onTriggered: root.view.folder.sortDescending = false }
        ContextMenuItem { text: qsTr("Descending"); radio: true; checkable: true; checked: root.view?.folder.sortDescending ?? false; onTriggered: root.view.folder.sortDescending = true }
    }

    // The menu of the strip's "..." button.
    ContextMenu {
        id: tabOptionsMenu
        ContextMenuItem { text: qsTr("New Tab"); shortcutText: "Ctrl+T"; onTriggered: root.newTab() }
        ContextMenuItem { text: qsTr("Reopen Closed Tab"); shortcutText: "Ctrl+Shift+T"; enabled: root.closedTabs.length > 0; onTriggered: root.reopenClosedTab() }
        ContextMenuSeparator {}
        ContextMenuItem { text: qsTr("Restore Tabs on Start"); checkable: true; checked: root.restoreTabs; onTriggered: root.setRestoreTabs(!root.restoreTabs) }
    }

    // The menu of a tab (right click on it).
    ContextMenu {
        id: tabMenu
        property int tab: -1
        ContextMenuItem { text: qsTr("New Tab"); onTriggered: root.newTab() }
        ContextMenuItem { text: qsTr("Duplicate Tab"); onTriggered: root.duplicateTab(tabMenu.tab) }
        ContextMenuSeparator {}
        ContextMenuItem { text: qsTr("Close Tab"); shortcutText: "Ctrl+W"; onTriggered: root.closeTab(tabMenu.tab) }
        ContextMenuItem { text: qsTr("Close Other Tabs"); enabled: tabsModel.count > 1; onTriggered: root.closeOthers(tabMenu.tab) }
        ContextMenuItem { text: qsTr("Close Tabs to the Right"); enabled: tabMenu.tab < tabsModel.count - 1; onTriggered: root.closeToTheRight(tabMenu.tab) }
        ContextMenuSeparator {}
        ContextMenuItem { text: qsTr("Reopen Closed Tab"); shortcutText: "Ctrl+Shift+T"; enabled: root.closedTabs.length > 0; onTriggered: root.reopenClosedTab() }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // The mouse's side buttons are Back and Forward in the tab shown.
        TapHandler {
            acceptedButtons: Qt.BackButton | Qt.ForwardButton
            onTapped: (point, button) => button === Qt.BackButton ? root.goBack() : root.goForward()
        }

        TelamonSidebar {
            id: sidebar
            Layout.fillHeight: true
            Layout.preferredWidth: Kirigami.Units.gridUnit * 12.5
            padding: Kirigami.Units.largeSpacing
            spacing: 2

            Repeater {
                model: root.places
                SidebarItem {
                    id: placeItem
                    required property var modelData
                    Layout.fillWidth: true
                    text: modelData.text
                    symbol: modelData.symbol
                    selected: root.place === modelData.key
                    // Ctrl+click and Ctrl+Enter open a new tab, as does a middle click.
                    onClicked: {
                        const target = StandardPlaces.place(modelData.key);
                        if (TabLogic.controlHeld()) {
                            root.openInNewTab(target);
                        } else {
                            root.navigate(target);
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        acceptedButtons: Qt.MiddleButton
                        onClicked: root.openInNewTab(StandardPlaces.place(placeItem.modelData.key))
                    }
                }
            }
        }

        Rectangle {
            Layout.fillHeight: true
            implicitWidth: 1
            color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            Item {
                Layout.fillWidth: true
                implicitHeight: tabBar.implicitHeight

                TabBar {
                    id: tabBar
                    anchors.fill: parent
                    model: tabsModel
                    currentIndex: root.currentIndex
                    onActivated: index => root.selectTab(index)
                    onCloseRequested: index => root.closeTab(index)
                    onNewRequested: root.newTab()
                    onMoved: (from, to) => root.moveTab(from, to)
                    onContextMenuRequested: (index, position) => {
                        tabMenu.tab = index;
                        tabMenu.popup(tabBar, position);
                    }

                    ToolbarButton {
                        symbol: Symbols.MoreVert
                        text: qsTr("Tab Options")
                        focusable: true
                        onClicked: tabOptionsMenu.popup(this, 0, height)
                    }
                }

                // Files dropped on a tab go into its folder. The framework's
                // TabBar takes no drops of its own (docs/DESIGN.md), so the tab
                // under the pointer is found among its items. Holding over
                // another tab for a moment shows it, to drop deeper.
                DropArea {
                    id: tabDrop
                    anchors.fill: parent
                    property int over: -1
                    function tabAt(item, x, y) {
                        for (let i = item.children.length - 1; i >= 0; --i) {
                            const c = item.children[i];
                            if (!c.visible) {
                                continue;
                            }
                            const p = item.mapToItem(c, x, y);
                            if (p.x < 0 || p.y < 0 || p.x > c.width || p.y > c.height) {
                                continue;
                            }
                            if (c.toolTipText !== undefined && c.index !== undefined) {
                                return c.index;
                            }
                            const found = tabAt(c, p.x, p.y);
                            if (found >= 0) {
                                return found;
                            }
                        }
                        return -1;
                    }
                    onPositionChanged: drag => {
                        const i = tabAt(tabBar, drag.x, drag.y);
                        if (i !== over) {
                            over = i;
                            springTimer.restart();
                        }
                    }
                    onEntered: drag => drag.accepted = drag.hasUrls
                    onExited: {
                        over = -1;
                        springTimer.stop();
                    }
                    onDropped: drop => {
                        const i = tabAt(tabBar, drop.x, drop.y);
                        const p = root.pageAt(i);
                        over = -1;
                        springTimer.stop();
                        if (p && drop.hasUrls) {
                            drop.accepted = true;
                            fileActions.drop(drop.urls, p.location);
                        }
                    }
                    Timer {
                        id: springTimer
                        interval: 800
                        onTriggered: if (tabDrop.over >= 0) root.selectTab(tabDrop.over)
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing

                ToolbarButton {
                    symbol: Symbols.ArrowBack
                    text: qsTr("Back")
                    shortcutText: "Alt+Left"
                    enabled: root.page?.canGoBack ?? false
                    focusable: true
                    onClicked: root.goBack()
                }
                ToolbarButton {
                    symbol: Symbols.ArrowForward
                    text: qsTr("Forward")
                    shortcutText: "Alt+Right"
                    enabled: root.page?.canGoForward ?? false
                    focusable: true
                    onClicked: root.goForward()
                }
                ToolbarButton {
                    symbol: Symbols.ArrowUpward
                    text: qsTr("Up")
                    focusable: true
                    onClicked: root.navigate(StandardPlaces.parentUrl(root.currentUrl))
                }
                // The location; a click turns it into a field for a path or URL.
                Text {
                    visible: !root.editingAddress
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    text: StandardPlaces.displayLocation(root.currentUrl)
                    color: Kirigami.Theme.textColor
                    MouseArea {
                        anchors.fill: parent
                        onClicked: root.startAddressEdit()
                    }
                }
                TelamonTextField {
                    id: address
                    visible: root.editingAddress
                    Layout.fillWidth: true
                    Keys.onEscapePressed: root.endAddressEdit()
                    onAccepted: root.goToAddress()
                    onActiveFocusChanged: if (!activeFocus && root.editingAddress) root.editingAddress = false
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.smallSpacing
                Layout.rightMargin: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing

                ToolbarButton {
                    symbol: Symbols.CreateNewFolder
                    text: qsTr("New Folder")
                    shortcutText: "Ctrl+Shift+N"
                    enabled: root.canWrite
                    focusable: true
                    onClicked: fileActions.newFolder()
                }
                ToolbarButton {
                    symbol: Symbols.ContentCut
                    text: qsTr("Cut")
                    shortcutText: "Ctrl+X"
                    enabled: root.hasSelection && root.canWrite
                    focusable: true
                    onClicked: fileActions.copy(root.selected, true)
                }
                ToolbarButton {
                    symbol: Symbols.ContentCopy
                    text: qsTr("Copy")
                    shortcutText: "Ctrl+C"
                    enabled: root.hasSelection
                    focusable: true
                    onClicked: fileActions.copy(root.selected, false)
                }
                ToolbarButton {
                    symbol: Symbols.ContentPaste
                    text: qsTr("Paste")
                    shortcutText: "Ctrl+V"
                    enabled: root.canWrite && fileActions.canPaste
                    focusable: true
                    onClicked: fileActions.paste()
                }
                ToolbarButton {
                    symbol: Symbols.DriveFileRenameOutline
                    text: qsTr("Rename")
                    shortcutText: "F2"
                    enabled: root.selected.length === 1 && root.canWrite
                    focusable: true
                    onClicked: fileActions.rename(root.selected[0])
                }
                ToolbarButton {
                    symbol: Symbols.Delete
                    text: qsTr("Move to Trash")
                    shortcutText: "Delete"
                    enabled: root.hasSelection && root.canWrite
                    focusable: true
                    onClicked: fileActions.trash(root.selected)
                }
                Item {
                    Layout.fillWidth: true
                }
                ToolbarButton {
                    symbol: Symbols.ViewList
                    text: qsTr("View")
                    focusable: true
                    onClicked: viewMenu.popup(this, 0, height)
                }
                ToolbarButton {
                    symbol: Symbols.Sort
                    text: qsTr("Sort")
                    focusable: true
                    onClicked: sortMenu.popup(this, 0, height)
                }
            }

            Text {
                visible: root.launchText.length > 0
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.smallSpacing
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                text: root.launchText
                color: Kirigami.Theme.negativeTextColor
            }

            // The tabs' pages sit here, one over another; only the current one shows.
            Item {
                id: pageHost
                Layout.fillWidth: true
                Layout.fillHeight: true
            }
        }
    }
}
