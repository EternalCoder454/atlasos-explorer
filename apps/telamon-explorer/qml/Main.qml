pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Explorer's window: the sidebar's places, the tab strip, the path bar, the
// command bar, the page of the tab shown (FilesTabPage) and the status line.
// Search and the panes come later (docs/ROADMAP.md).
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
    // What the last launch asked for that Explorer refused, shown over the view.
    property string launchText
    // Why the address typed in the path bar was refused, shown under it.
    property string addressError
    readonly property bool editingAddress: pathBar.editing
    // A text field has the keyboard.
    readonly property bool inTextField: pathBar.editing || searchField.activeFocus || searchBar.activeFocus || (view ? view.renaming : false)
    // Keys that mean something to a text field, or to Quick Look (which shows
    // a file and must not act on it), are not the window's.
    readonly property bool typing: inTextField || quickLook.opened
    onInTextFieldChanged: {
        if (inTextField) {
            quickLook.close();
        }
    }
    onViewChanged: quickLook.close()
    // Quick Look is over the window: a player in the gallery stops meanwhile.
    Binding {
        target: root.view
        property: "covered"
        value: quickLook.opened
        when: root.view !== null
    }
    // The tab's search, and whether its results are what the view shows.
    readonly property var search: page ? page.search : null
    readonly property bool searching: view ? view.folder.searching : false
    readonly property var selected: view ? view.selectedUrls : []
    readonly property bool hasSelection: selected.length > 0
    readonly property bool canWrite: view ? view.folder.canWrite : false
    // The folder shown is the Trash (or a folder in it), and whether it is the top of the Trash.
    readonly property bool inTrash: view ? view.folder.inTrash : false
    readonly property bool inTrashTop: view ? view.folder.trashTop : false
    // {files, folders, bytes} of the selection, for the status line.
    readonly property var selectionStats: {
        if (!view) {
            return ({
                    "files": 0,
                    "folders": 0,
                    "bytes": 0
                });
        }
        view.selRevision;
        view.folder.count;
        return view.folder.selectionStats(view.selectedRows());
    }
    // Free space of the disk the folder is on (-1: not known), asked of a
    // worker when the folder changes and a second after its items change.
    property real freeBytes: -1
    property int freeSerial: -1
    function refreshFreeSpace() {
        freeSerial = LocationLogic.queryFreeSpace(currentUrl);
    }
    onCurrentUrlChanged: refreshFreeSpace()

    Connections {
        target: LocationLogic
        function onFreeSpaceReady(serial, bytes) {
            if (serial === root.freeSerial) {
                root.freeBytes = bytes;
            }
        }
    }
    Connections {
        target: root.view ? root.view.folder : null
        function onCountChanged() {
            freeTimer.restart();
        }
    }
    Timer {
        id: freeTimer
        interval: 1000
        onTriggered: root.refreshFreeSpace()
    }

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
        onOpenLocationRequested: files => root.openFileLocation(files)
        onNamePromptRequested: request => namePrompt.ask(request)
        onRenameRequested: urls => root.renameItems(urls)
        onCreatedItem: url => root.editNewItem(url)
        onResultsReady: urls => root.selectResults(urls)
        onDeleteRequested: (urls, text) => {
            deleteDialog.urls = urls;
            deleteDialog.text = text;
            deleteDialog.open();
        }
        // A drop with no key held asks what to do with the files.
        onDropMenuRequested: (urls, destination, x, y) => {
            dropMenu.urls = urls;
            dropMenu.destination = destination;
            Qt.callLater(() => dropMenu.popup(root.contentItem, x, y));
        }
        // Files in the results were trashed, moved or renamed: drop the ones
        // that are gone now, and look again once the index has caught up.
        onJobFinished: {
            // Any tab may be the one whose results were changed.
            for (const id in root.pages) {
                const f = root.pages[id].view.folder;
                if (f.searching) {
                    f.pruneSearchResults();
                    searchAgain.restart();
                }
            }
        }
    }
    // F2 and Rename: one item is edited where it is shown, several open Batch Rename.
    function renameItems(urls) {
        if (!view || urls.length === 0) {
            return;
        }
        if (urls.length === 1) {
            view.startRename(urls[0]);
        } else if (!view.folder.searching) {
            batchRename.ask(urls);
        }
    }
    // A new folder or file was made in the folder shown: it is selected and its
    // name is edited at once.
    function editNewItem(url) {
        const here = view ? view.url.toString().replace(/\/+$/, "") : "";
        if (!page || StandardPlaces.parentUrl(url).toString().replace(/\/+$/, "") !== here) {
            return;
        }
        page.showItems([url]);
        view.renameWhenListed(url);
    }
    // What an extraction or compression made is selected, where it is shown.
    function selectResults(urls) {
        if (!urls || urls.length === 0) {
            return;
        }
        const folder = StandardPlaces.parentUrl(urls[0]).toString().replace(/\/+$/, "");
        for (const id in root.pages) {
            const p = root.pages[id];
            if (p.loaded && p.location.toString().replace(/\/+$/, "") === folder) {
                p.showItems(urls);
            }
        }
    }
    // What the operation queue says in a line, and what it refuses.
    Connections {
        target: fileActions.operations
        function onMessage(text) {
            root.toast(text);
        }
        function onRefused(title, text) {
            refusedDialog.title = title;
            refusedDialog.text = text;
            refusedDialog.open();
        }
    }

    // Delete for good asks first, and says it can't be undone. Cancel is the default.
    ConfirmDialog {
        id: deleteDialog
        property var urls: []
        title: qsTr("Delete for Good?")
        acceptText: qsTr("Delete")
        rejectText: qsTr("Cancel")
        destructive: true
        defaultButton: "reject"
        focusReject: true
        onAccepted: fileActions.confirmDelete(urls)
    }

    // A copy or move that was refused up front, with the reason.
    ConfirmDialog {
        id: refusedDialog
        acceptText: qsTr("OK")
        showReject: false
    }

    // Move Here, Copy Here, Link Here for files dropped without a key held.
    ContextMenu {
        id: dropMenu
        property var urls: []
        property url destination
        ContextMenuItem {
            text: qsTr("Move Here")
            onTriggered: fileActions.dropWith(dropMenu.urls, dropMenu.destination, "move")
        }
        ContextMenuItem {
            text: qsTr("Copy Here")
            onTriggered: fileActions.dropWith(dropMenu.urls, dropMenu.destination, "copy")
        }
        ContextMenuItem {
            text: qsTr("Link Here")
            onTriggered: fileActions.dropWith(dropMenu.urls, dropMenu.destination, "link")
        }
        ContextMenuSeparator {}
        ContextMenuItem {
            text: qsTr("Cancel")
        }
    }

    // Name conflicts, and what else a job asks, in the Telamon look.
    ConflictDialog {
        queue: fileActions.operations
    }
    ProblemDialog {
        queue: fileActions.operations
    }

    // The name of one item, where it can't be edited in place (the gallery, search results).
    NamePrompt {
        id: namePrompt
        actions: fileActions
    }
    // Several items renamed together.
    BatchRenameDialog {
        id: batchRename
        actions: fileActions
    }
    // Connect to Server (Ctrl+Shift+K, and in the sidebar's Network section).
    ConnectDialog {
        id: connectDialog
        onConnectRequested: target => {
            ServerLogic.remember(target);
            root.navigate(target);
        }
        onAddToSidebarRequested: target => {
            ServerLogic.remember(target);
            PlacesLogic.pinServer(target);
        }
    }

    // The context menus of the items and of the folder's empty space.
    FileMenu {
        id: fileMenu
        actions: fileActions
    }
    BackgroundMenu {
        id: backgroundMenu
        actions: fileActions
        win: root
    }
    function showContextMenu(urls, anchor, x, y) {
        if (urls.length > 0) {
            fileMenu.openFor(urls, anchor, x, y);
        } else {
            backgroundMenu.openFor(anchor, x, y);
        }
    }

    Timer {
        id: searchAgain
        interval: 900
        onTriggered: {
            for (const id in root.pages) {
                const s = root.pages[id].search;
                if (s.active && !s.live) {
                    s.rerun();
                }
            }
        }
    }

    Component {
        id: pageComponent
        FilesTabPage {
            actions: fileActions
        }
    }

    // ---- Places ----
    // The sidebar's sections, from KIO's places (user-places.xbel, drives from
    // Solid); PlacesLogic does what the sidebar asks of them.
    PlacesModel {
        id: favouritesModel
        section: PlacesLogic.Favourites
    }
    PlacesModel {
        id: drivesModel
        section: PlacesLogic.Drives
    }
    PlacesModel {
        id: networkModel
        section: PlacesLogic.Network
    }
    PlacesModel {
        id: trashModel
        section: PlacesLogic.Trash
    }

    // A place in the sidebar: its menu opens at the pointer.
    component SidebarPlace: PlaceItem {
        id: me
        Layout.fillWidth: true
        current: root.currentUrl
        onMenuRequested: pos => root.showPlaceMenu(me, me.mapToItem(sidebar, pos.x, pos.y))
    }

    component SectionLabel: Text {
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.smallSpacing
        leftPadding: TelamonStyle.spacingLarge
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        font.weight: Font.Medium
        textFormat: Text.PlainText
        color: TelamonStyle.textMuted
    }

    Connections {
        target: PlacesLogic
        function onOpenRequested(url, newTab) {
            if (newTab) {
                root.openInNewTab(url);
            } else {
                root.navigate(url);
            }
        }
        function onMessage(text) {
            root.toast(text);
        }
        function onEmptyTrashAsk(count, text) {
            emptyTrashDialog.text = text;
            emptyTrashDialog.open();
        }
        function onFilesDropped(files, destination, copy) {
            fileActions.dropTo(files, destination, copy);
        }
        function onTrashDropped(urls) {
            fileActions.trash(urls);
        }
    }

    // Something dropped on a place of the sidebar: a pin dragged there takes
    // its position; anything else is for PlacesLogic (a folder is pinned,
    // files go into the folder, the Trash trashes).
    function dropOnPlace(entry, drop) {
        if (entry.placeKey === undefined) {
            return;
        }
        if (drop.source && drop.source.placeKey !== undefined) {
            PlacesLogic.moveTo(drop.source.placeKey, entry.placeKey);
            return;
        }
        if (!drop.hasUrls) {
            return;
        }
        // Moves unless Ctrl is held, as on the path bar. The move is made
        // here, so the source is told it was a copy and leaves the files alone.
        const canMove = (drop.supportedActions & Qt.MoveAction) !== 0;
        const canCopy = (drop.supportedActions & Qt.CopyAction) !== 0;
        const copy = canCopy && (!canMove || fileActions.copyKeyHeld());
        drop.accept(Qt.CopyAction);
        PlacesLogic.handleDrop(drop.urls, entry.placeKey, copy);
    }
    function showPlaceMenu(entry, pos) {
        if (entry.placeKey === undefined) {
            return;
        }
        placeMenu.key = entry.placeKey;
        placeMenu.info = PlacesLogic.menuFor(entry.placeKey);
        // After the click that asked for it is over: a popup opened while its
        // own button release is delivered closes with it.
        Qt.callLater(() => placeMenu.popup(sidebar, pos.x, pos.y));
    }

    // The menu of a place (right click, or the Menu key on it).
    ContextMenu {
        id: placeMenu
        property string key
        property var info: ({})
        ContextMenuItem {
            text: qsTr("Open")
            visible: placeMenu.info.open === true
            onTriggered: PlacesLogic.open(placeMenu.key, false)
        }
        ContextMenuItem {
            text: qsTr("Open in New Tab")
            visible: placeMenu.info.newTab === true
            onTriggered: PlacesLogic.open(placeMenu.key, true)
        }
        ContextMenuItem {
            text: qsTr("Mount")
            visible: placeMenu.info.mount === true
            onTriggered: PlacesLogic.open(placeMenu.key, false)
        }
        ContextMenuItem {
            text: placeMenu.info.removable === true ? qsTr("Eject") : qsTr("Unmount")
            visible: placeMenu.info.unmount === true
            onTriggered: PlacesLogic.unmount(placeMenu.key)
        }
        ContextMenuItem {
            text: qsTr("Open in Disks")
            visible: placeMenu.info.openInDisks === true
            onTriggered: PlacesLogic.openInDisks(placeMenu.key)
        }
        ContextMenuItem {
            text: qsTr("Empty Trash\u2026")
            visible: placeMenu.info.kind === PlacesLogic.TrashPlace
            enabled: placeMenu.info.emptyTrash === true
            onTriggered: PlacesLogic.requestEmptyTrash()
        }
        ContextMenuSeparator {
            visible: placeMenu.info.rename === true || placeMenu.info.hide === true || placeMenu.info.unhide === true || placeMenu.info.remove === true
        }
        ContextMenuItem {
            text: qsTr("Rename\u2026")
            visible: placeMenu.info.rename === true
            onTriggered: renameDialog.openFor(placeMenu.key)
        }
        ContextMenuItem {
            text: qsTr("Hide")
            visible: placeMenu.info.hide === true
            onTriggered: PlacesLogic.setHidden(placeMenu.key, true)
        }
        ContextMenuItem {
            text: qsTr("Show in Sidebar")
            visible: placeMenu.info.unhide === true
            onTriggered: PlacesLogic.setHidden(placeMenu.key, false)
        }
        ContextMenuItem {
            text: qsTr("Remove from Sidebar")
            visible: placeMenu.info.remove === true
            destructive: true
            onTriggered: PlacesLogic.remove(placeMenu.key)
        }
    }

    TelamonDialog {
        id: renameDialog
        property string key
        title: qsTr("Rename Place")
        preferredWidth: Kirigami.Units.gridUnit * 22
        function openFor(placeKey) {
            key = placeKey;
            renameField.text = PlacesLogic.nameOf(placeKey);
            open();
        }
        function commit() {
            if (renameField.text.trim().length > 0) {
                PlacesLogic.rename(key, renameField.text);
                close();
            }
        }
        onOpened: {
            renameField.forceActiveFocus();
            renameField.selectAll();
        }
        footerContent: [
            SecondaryButton {
                text: qsTr("Cancel")
                onClicked: renameDialog.close()
            },
            PrimaryButton {
                text: qsTr("Rename")
                enabled: renameField.text.trim().length > 0
                onClicked: renameDialog.commit()
            }
        ]
        TelamonTextField {
            id: renameField
            Layout.fillWidth: true
            maximumLength: 80
            onAccepted: renameDialog.commit()
        }
    }

    // Empty Trash asks first, naming how much goes. Cancel is the default.
    ConfirmDialog {
        id: emptyTrashDialog
        title: qsTr("Empty Trash?")
        acceptText: qsTr("Empty Trash")
        rejectText: qsTr("Cancel")
        destructive: true
        defaultButton: "reject"
        focusReject: true
        onAccepted: fileActions.emptyTrash()
    }

    // Restore found folders that are gone: they are made again only if the user says so.
    ConfirmDialog {
        id: restoreDialog
        property bool answered: false
        acceptText: qsTr("Create and Restore")
        rejectText: qsTr("Cancel")
        defaultButton: "reject"
        focusReject: true
        onAccepted: {
            answered = true;
            fileActions.confirmRestore();
        }
        onClosed: {
            if (!answered) {
                fileActions.cancelRestore();
            }
            answered = false;
        }
    }
    Connections {
        target: fileActions
        function onRestoreAsk(title, text) {
            restoreDialog.title = title;
            restoreDialog.text = text;
            restoreDialog.answered = false;
            restoreDialog.open();
        }
    }

    // "Empty items older than N days": turning it on (or lowering the days so
    // that something goes now) asks first. Cancel is the default.
    ConfirmDialog {
        id: autoEmptyDialog
        property bool answered: false
        title: qsTr("Empty Old Trash Items Automatically?")
        acceptText: qsTr("Turn On")
        rejectText: qsTr("Cancel")
        destructive: true
        defaultButton: "reject"
        focusReject: true
        onAccepted: {
            answered = true;
            TrashLogic.confirm();
        }
        onClosed: {
            if (!answered) {
                TrashLogic.cancel();
            }
            answered = false;
        }
    }
    Connections {
        target: TrashLogic
        function onConfirmRequested(text) {
            autoEmptyDialog.text = text;
            autoEmptyDialog.answered = false;
            autoEmptyDialog.open();
        }
        function onEmptyOldRequested(days) {
            fileActions.emptyOldTrash(days);
        }
    }

    // The Trash setting, for now (the Settings window comes in a later wave).
    function openTrashSettings() {
        trashSettingsDialog.open();
    }
    TelamonDialog {
        id: trashSettingsDialog
        title: qsTr("Trash")
        preferredWidth: Kirigami.Units.gridUnit * 30
        footerContent: [
            SecondaryButton {
                text: qsTr("Close")
                onClicked: trashSettingsDialog.close()
            }
        ]
        TrashAutoEmpty {
            Layout.fillWidth: true
        }
        Text {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            text: qsTr("Files deletes what has been in the Trash for longer than that, for good, when it starts and once a day while it is open. Newer items are never touched.")
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
        }
    }

    // ---- Address ----
    function goToAddress(text) {
        const r = fileActions.parseAddress(text);
        if (r.ok) {
            addressError = "";
            launchText = "";
            const target = Qt.url(r.text);
            // A server typed here is one of the recent servers too (never with a password).
            ServerLogic.remember(target);
            navigate(target);
            pathBar.endEdit();
        } else {
            addressError = r.text;
        }
    }
    function toggleHidden() {
        if (!view) {
            return;
        }
        const on = !view.folder.showHidden;
        // Every tab, so switching tabs doesn't change what is shown.
        for (const id in pages) {
            pages[id].view.folder.showHidden = on;
        }
        fileActions.saveShowHidden(on);
    }

    // ---- Quick Look, the preview pane and zoom ----
    // Space on the tab shown: a large preview of the selected file.
    function showQuickLook(from) {
        if (from === page && view && !typing) {
            quickLook.openFor(view);
        }
    }
    // Ctrl+plus, Ctrl+minus (steps) and Ctrl+0 (0): the icons' size in the
    // Icons view, the rows' height in the others.
    function zoom(steps) {
        if (view) {
            view.zoom(steps);
        }
    }
    // The Settings switch for thumbnails of files on servers (the Settings
    // window of a later wave takes it over): every tab asks for its thumbnails again.
    function setPreviewRemote(on) {
        ServerLogic.previewRemote = on;
        for (const id in pages) {
            pages[id].view.folder.thumbnailsChanged();
        }
    }
    // "Use the Same View for Every Folder" on or off: the view shown stays.
    function setSameView(on) {
        ViewMemory.sameForAll = on;
        if (view) {
            view.remember();
        }
    }
    // Whether the folder shown has its own remembered view (`revision` only
    // makes the menu read it again after a change).
    function hasRememberedView(revision) {
        return view ? ViewMemory.hasEntry(view.folder.url) : false;
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
    // ---- Search ----
    // Puts the keyboard in the search field (which opens the search row).
    function focusSearch() {
        pathBar.endEdit();
        searchField.forceActiveFocus(Qt.ShortcutFocusReason);
        searchField.selectAll();
    }
    // Open File Location on search results: the folder they are in is shown in
    // this tab with them selected, replacing the search; results in other
    // folders open in tabs behind it.
    function openFileLocation(urls) {
        if (!page || urls.length === 0) {
            return;
        }
        const groups = [];
        for (const u of urls) {
            const folder = StandardPlaces.parentUrl(u);
            const key = folder.toString();
            let g = groups.find(x => x.key === key);
            if (!g) {
                g = {
                    "key": key,
                    "folder": folder,
                    "items": []
                };
                groups.push(g);
            }
            g.items.push(u);
        }
        const shown = page;
        freshStart = false;
        shown.navigate(groups[0].folder);
        shown.showItems(groups[0].items);
        for (let i = 1; i < groups.length && i < 8; ++i) {
            addTab(groups[i].folder, {
                "background": true,
                "select": groups[i].items
            });
        }
    }
    // The menu of Back (or Forward): the last places of the tab, nearest first.
    property bool menuOpenedByPress: false
    function showHistory(forward, anchor) {
        if (!page) {
            return;
        }
        const places = forward ? page.forwardPlaces() : page.backPlaces();
        if (places.length === 0) {
            return;
        }
        menuOpenedByPress = true;
        historyMenu.forward = forward;
        historyMenu.places = places;
        historyMenu.popup(anchor, 0, anchor.height + Kirigami.Units.smallSpacing);
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
        pathBar.endEdit();
        p.focusContent();
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
            "startViewMode": o.viewMode || "",
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
        p.openLocation.connect(urls => root.openFileLocation(urls));
        p.quickLookRequested.connect(() => root.showQuickLook(p));
        p.contextMenuRequested.connect((urls, x, y) => root.showContextMenu(urls, p.view, x, y));
        p.navigated.connect(() => root.freshStart = false);
        // A folder the user went to is counted for Home's Frequent Folders.
        p.navigated.connect(() => HomeLogic.visited(p.location));
        p.connectRequested.connect(() => connectDialog.ask());
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
        addTab(StandardPlaces.place("homepage"), {});
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
        removeTab(i);
        selectTab(next);
    }
    // Removes the tab at `i` without showing another one first.
    function removeTab(i) {
        const p = pageAt(i);
        if (!p) {
            return;
        }
        closedTabs = closedTabs.concat([{
                    "url": p.location,
                    "viewMode": p.view.viewMode,
                    "back": p.backStack.slice(-TabLogic.maxHistory),
                    "forward": p.forwardStack.slice(-TabLogic.maxHistory),
                    "index": i
                }]).slice(-TabLogic.maxClosed);
        if (p === page) {
            page = null;
        }
        tabsModel.remove(i);
        delete pages[p.tabId];
        p.visible = false;
        p.destroy();
    }
    // Closes every tab but `keep`, which is then shown.
    function closeOthers(keep) {
        if (!pageAt(keep)) {
            return;
        }
        freshStart = false;
        for (let n = tabsModel.count - 1; n >= 0; --n) {
            if (n !== keep) {
                removeTab(n);
                if (n < keep) {
                    keep--;
                }
            }
        }
        selectTab(keep);
    }
    // Closes the tabs after `i`; the tab shown stays unless it was one of them.
    function closeToTheRight(i) {
        if (!pageAt(i)) {
            return;
        }
        freshStart = false;
        const wasShown = currentIndex;
        for (let n = tabsModel.count - 1; n > i; --n) {
            removeTab(n);
        }
        selectTab(wasShown > i ? i : wasShown);
    }
    function reopenClosedTab() {
        if (closedTabs.length === 0) {
            return;
        }
        const t = closedTabs[closedTabs.length - 1];
        freshStart = false;
        // Only a tab that was opened leaves the list.
        if (addTab(t.url, {
            "index": t.index,
            "viewMode": t.viewMode,
            "back": t.back,
            "forward": t.forward
        })) {
            closedTabs = closedTabs.slice(0, -1);
        }
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
        PreviewLogic.rowDefault = Kirigami.Units.gridUnit * 2;
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
            addTab(StandardPlaces.place("homepage"), {});
        }
        TrashLogic.begin();
    }

    // The keys. Those that mean something to a text field are off while the
    // address is being typed.
    Shortcut { sequence: "Ctrl+C"; enabled: !root.typing && root.hasSelection; onActivated: fileActions.copy(root.selected, false) }
    Shortcut { sequence: "Ctrl+X"; enabled: !root.typing && root.hasSelection && root.canWrite; onActivated: fileActions.copy(root.selected, true) }
    Shortcut { sequence: "Ctrl+Shift+C"; enabled: !root.typing && root.hasSelection; onActivated: fileActions.copyPath(root.selected) }
    Shortcut { sequence: "Ctrl+V"; enabled: !root.typing && root.canWrite && !root.searching; onActivated: fileActions.paste() }
    Shortcut { sequence: "Ctrl+Z"; enabled: !root.typing; onActivated: fileActions.undo() }
    Shortcut { sequences: ["Ctrl+Shift+Z", "Ctrl+Y"]; enabled: !root.typing; onActivated: fileActions.redo() }
    // In the Trash there is nothing to trash: Delete deletes (after asking).
    Shortcut { sequence: "Delete"; enabled: !root.typing && root.hasSelection && (root.canWrite || root.inTrash); onActivated: root.inTrash ? fileActions.deleteForGood(root.selected) : fileActions.trash(root.selected) }
    Shortcut { sequence: "Shift+Delete"; enabled: !root.typing && root.hasSelection && (root.canWrite || root.inTrash); onActivated: fileActions.deleteForGood(root.selected) }
    Shortcut { sequence: "Ctrl+Shift+N"; enabled: root.canWrite && !root.searching && !quickLook.opened; onActivated: fileActions.newFolder() }
    Shortcut { sequence: "Ctrl+H"; enabled: !root.typing; onActivated: root.toggleHidden() }
    Shortcut { sequence: "Shift+F4"; enabled: !quickLook.opened; onActivated: fileActions.openTerminal() }
    Shortcut { sequences: ["Ctrl+L", "F4", "F6", "Alt+D"]; onActivated: pathBar.startEdit() }
    Shortcut { sequences: ["Ctrl+F", "Ctrl+E"]; onActivated: root.focusSearch() }
    Shortcut { sequence: "Alt+P"; enabled: !quickLook.opened; onActivated: PreviewLogic.paneShown = !PreviewLogic.paneShown }
    Shortcut { sequences: ["Ctrl++", "Ctrl+=", "Ctrl+Plus"]; enabled: !root.typing; onActivated: root.zoom(1) }
    Shortcut { sequences: ["Ctrl+-", "Ctrl+Minus"]; enabled: !root.typing; onActivated: root.zoom(-1) }
    Shortcut { sequence: "Ctrl+0"; enabled: !root.typing; onActivated: root.zoom(0) }
    Shortcut { sequence: "Alt+Left"; enabled: !root.typing; onActivated: root.goBack() }
    Shortcut { sequence: "Alt+Right"; enabled: !root.typing; onActivated: root.goForward() }
    Shortcut { sequences: ["F5", "Ctrl+R"]; enabled: !root.typing; onActivated: { if (root.view) root.view.folder.refresh(); } }
    Shortcut { sequence: "Ctrl+Shift+K"; enabled: !quickLook.opened && !connectDialog.opened; onActivated: connectDialog.ask() }
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

    ViewMenu {
        id: viewMenu
        win: root
    }

    SortMenu {
        id: sortMenu
        win: root
    }

    // The places Back or Forward would go to: picking one jumps there.
    ContextMenu {
        id: historyMenu
        property bool forward: false
        property var places: []
        Instantiator {
            model: historyMenu.places
            delegate: ContextMenuItem {
                required property var modelData
                text: StandardPlaces.displayLocation(modelData.url).replace(/&/g, "&&")
                onTriggered: {
                    if (historyMenu.forward) {
                        root.page.goForwardBy(modelData.steps);
                    } else {
                        root.page.goBackBy(modelData.steps);
                    }
                }
            }
            onObjectAdded: (index, object) => historyMenu.insertItem(index, object)
            onObjectRemoved: (index, object) => historyMenu.removeItem(object)
        }
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
            dropEnabled: true
            onContextMenuRequested: (entry, pos) => root.showPlaceMenu(entry, pos)
            onDropped: (entry, drop) => root.dropOnPlace(entry, drop)

            Repeater {
                model: favouritesModel
                delegate: SidebarPlace {}
            }

            SectionLabel {
                visible: drivesRepeater.count > 0
                text: qsTr("Drives")
            }
            Repeater {
                id: drivesRepeater
                model: drivesModel
                delegate: SidebarPlace {}
            }

            SectionLabel {
                text: qsTr("Network")
            }
            Repeater {
                id: networkRepeater
                model: networkModel
                delegate: SidebarPlace {}
            }
            SidebarItem {
                Layout.fillWidth: true
                text: qsTr("Connect to Server…")
                symbol: Symbols.AddLink
                Accessible.description: "Ctrl+Shift+K"
                onClicked: connectDialog.ask()
            }

            // Places that were hidden come back from here.
            SidebarItem {
                Layout.fillWidth: true
                visible: PlacesLogic.hiddenCount > 0
                text: PlacesLogic.showHidden ? qsTr("Hide Hidden Places") : qsTr("Show Hidden Places (%1)").arg(PlacesLogic.hiddenCount)
                symbol: PlacesLogic.showHidden ? Symbols.VisibilityOff : Symbols.Visibility
                opacity: 0.75
                onClicked: PlacesLogic.showHidden = !PlacesLogic.showHidden
            }

            // The Trash stays under the list.
            footer: [
                Repeater {
                    model: trashModel
                    delegate: SidebarPlace {}
                }
            ]
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
                // Above the command bar: the path bar's drop hint hangs below it.
                z: 2

                // A long press or a right click lists the last places of the tab.
                ToolbarButton {
                    id: backButton
                    symbol: Symbols.ArrowBack
                    text: qsTr("Back")
                    shortcutText: "Alt+Left"
                    enabled: root.page?.canGoBack ?? false
                    focusable: true
                    onPressed: root.menuOpenedByPress = false
                    onClicked: {
                        if (!root.menuOpenedByPress) {
                            root.goBack();
                        }
                    }
                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: root.showHistory(false, backButton)
                    }
                    TapHandler {
                        acceptedButtons: Qt.LeftButton
                        onLongPressed: root.showHistory(false, backButton)
                    }
                }
                ToolbarButton {
                    id: forwardButton
                    symbol: Symbols.ArrowForward
                    text: qsTr("Forward")
                    shortcutText: "Alt+Right"
                    enabled: root.page?.canGoForward ?? false
                    focusable: true
                    onPressed: root.menuOpenedByPress = false
                    onClicked: {
                        if (!root.menuOpenedByPress) {
                            root.goForward();
                        }
                    }
                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: root.showHistory(true, forwardButton)
                    }
                    TapHandler {
                        acceptedButtons: Qt.LeftButton
                        onLongPressed: root.showHistory(true, forwardButton)
                    }
                }
                ToolbarButton {
                    symbol: Symbols.ArrowUpward
                    text: qsTr("Up")
                    focusable: true
                    onClicked: root.navigate(StandardPlaces.parentUrl(root.currentUrl))
                }
                // The path: segments to click, a menu per chevron, drops on a
                // segment; a click on the empty part edits it as text.
                PathBar {
                    id: pathBar
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    location: root.currentUrl
                    showHidden: root.view?.folder.showHidden ?? false
                    actions: fileActions
                    onNavigateRequested: target => root.navigate(target)
                    onOpenInNewTabRequested: target => root.openInNewTab(target)
                    onAddressAccepted: text => root.goToAddress(text)
                    onAddressEdited: root.addressError = ""
                    onEditEnded: {
                        root.addressError = "";
                        if (root.page) {
                            root.page.focusContent();
                        }
                    }
                }
                // Search: type to see the best matches at once (Ctrl+F or Ctrl+E).
                SearchField {
                    id: searchField
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 16
                    Layout.minimumWidth: Kirigami.Units.gridUnit * 8
                    enabled: root.page !== null && root.page.pageKind !== "network"
                    placeholderText: root.search && root.search.scope === 1 ? qsTr("Search Everywhere") : qsTr("Search %1").arg(root.page ? root.page.title : "")
                    text: root.search ? root.search.text : ""
                    Accessible.name: qsTr("Search")
                    onTextChanged: {
                        if (root.search && root.search.text !== text) {
                            root.search.text = text;
                        }
                    }
                    onActiveFocusChanged: {
                        if (activeFocus && root.search) {
                            root.search.warm();
                        }
                    }
                    // Escape ends the search and returns to the folder; Down and
                    // Enter move to the results.
                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Escape) {
                            if (root.search) {
                                root.search.clear();
                            }
                            if (root.page) {
                                root.page.focusContent();
                            }
                            event.accepted = true;
                        } else if (event.key === Qt.Key_Down || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                            const v = root.view;
                            if (v) {
                                v.forceActiveFocus();
                                if (v.folder.count > 0 && v.selectedUrls.length === 0) {
                                    v.chooseRow(0, 0, false);
                                }
                            }
                            event.accepted = true;
                        }
                    }
                }
            }

            // Where the search looks, its filters and how it is going.
            SearchBar {
                id: searchBar
                Layout.fillWidth: true
                search: root.search
                field: searchField
                onCloseRequested: {
                    if (root.search) {
                        root.search.clear();
                    }
                    if (root.page) {
                        root.page.focusContent();
                    }
                }
            }

            // A folder on FTP, plain WebDAV or NFS: what it carries can be read on the way.
            InfoBanner {
                Layout.fillWidth: true
                type: "warning"
                shown: root.view ? root.view.folder.securityNote.length > 0 : false
                text: qsTr("%1. What you open or copy on this server can be read by others on the network.").arg(root.view ? root.view.folder.securityNote : "")
            }

            Text {
                visible: root.addressError.length > 0
                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.smallSpacing * 2
                Layout.rightMargin: Kirigami.Units.smallSpacing
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                text: root.addressError
                color: Kirigami.Theme.negativeTextColor
                Accessible.role: Accessible.AlertMessage
                Accessible.name: root.addressError
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: Kirigami.Units.smallSpacing
                Layout.rightMargin: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing

                // An archive opened as a folder is read-only: this takes everything out.
                ToolbarButton {
                    visible: root.view ? root.view.folder.inArchive : false
                    symbol: Symbols.Unarchive
                    text: qsTr("Extract")
                    toolTipText: qsTr("Extract Everything in This Archive")
                    display: T.AbstractButton.TextBesideIcon
                    focusable: true
                    onClicked: fileActions.extractViewed()
                }
                // In the Trash: put the items back, delete them for good, or empty it.
                ToolbarButton {
                    visible: root.inTrash
                    symbol: Symbols.RestoreFromTrash
                    text: qsTr("Restore")
                    toolTipText: qsTr("Restore to Where It Was")
                    display: T.AbstractButton.TextBesideIcon
                    enabled: root.hasSelection && root.inTrashTop
                    focusable: true
                    onClicked: fileActions.restore(root.selected)
                }
                TelamonButton {
                    visible: root.inTrash
                    variant: TelamonButton.Destructive
                    symbol: Symbols.DeleteForever
                    text: qsTr("Empty Trash")
                    enabled: PlacesLogic.trashCount > 0
                    focusPolicy: Qt.StrongFocus
                    onClicked: PlacesLogic.requestEmptyTrash()
                }
                ToolbarButton {
                    visible: root.inTrash
                    symbol: Symbols.Delete
                    text: qsTr("Delete")
                    toolTipText: qsTr("Delete for Good")
                    shortcutText: "Delete"
                    enabled: root.hasSelection
                    focusable: true
                    onClicked: fileActions.deleteForGood(root.selected)
                }
                ToolbarButton {
                    visible: !root.inTrash
                    symbol: Symbols.CreateNewFolder
                    text: qsTr("New Folder")
                    shortcutText: "Ctrl+Shift+N"
                    enabled: root.canWrite && !root.searching
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
                    visible: !root.inTrash
                    symbol: Symbols.ContentPaste
                    text: qsTr("Paste")
                    shortcutText: "Ctrl+V"
                    enabled: root.canWrite && !root.searching && fileActions.canPaste
                    focusable: true
                    onClicked: fileActions.paste()
                }
                ToolbarButton {
                    visible: !root.inTrash
                    symbol: Symbols.DriveFileRenameOutline
                    text: qsTr("Rename")
                    shortcutText: "F2"
                    enabled: root.canWrite && (root.selected.length === 1 || (root.selected.length > 1 && !root.searching))
                    focusable: true
                    onClicked: fileActions.rename(root.selected)
                }
                ToolbarButton {
                    visible: !root.inTrash
                    symbol: Symbols.Delete
                    text: qsTr("Move to Trash")
                    shortcutText: "Delete"
                    enabled: root.hasSelection && root.canWrite
                    focusable: true
                    onClicked: fileActions.trash(root.selected)
                }
                // Cut, and not pasted yet.
                Text {
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    visible: fileActions.cutCount > 0
                    text: fileActions.cutCount === 1 ? qsTr("1 item waiting to move") : qsTr("%1 items waiting to move").arg(fileActions.cutCount)
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                    textFormat: Text.PlainText
                    Accessible.role: Accessible.StaticText
                    Accessible.name: text
                }
                Item {
                    Layout.fillWidth: true
                }
                OperationsButton {
                    queue: fileActions.operations
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

            StatusLine {
                Layout.fillWidth: true
                folder: root.view?.folder ?? null
                search: root.search
                selection: root.selectionStats
                freeBytes: root.freeBytes
            }
        }

        // The preview pane (Alt+P), on the right.
        Rectangle {
            Layout.fillHeight: true
            implicitWidth: 1
            visible: PreviewLogic.paneShown
            color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
        }
        PreviewPane {
            Layout.fillHeight: true
            Layout.preferredWidth: Kirigami.Units.gridUnit * 20
            visible: PreviewLogic.paneShown
            view: PreviewLogic.paneShown ? root.view : null
            covered: quickLook.opened
        }
    }

    // Quick Look, over everything in the window.
    QuickLook {
        id: quickLook
        onOpenFile: row => {
            if (root.view) {
                root.view.activateRow(row);
            }
        }
    }
}
