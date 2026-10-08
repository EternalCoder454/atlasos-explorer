pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQml.Models
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A folder shown as details, icons, a compact list, columns or a gallery:
// selection, keyboard navigation and type-ahead live here, the views only draw
// rows. Opening a file is not done here: `openRequested` hands the URLs to
// whoever owns the trust prompts. How the folder is shown (view, sort, icon
// size, grouping) is remembered for it (ViewMemory) and brought back when the
// tab goes to it.
FocusScope {
    id: top

    // One of Files' own pages (Home, Network) is shown in place of the
    // folder: the window draws it, and this view stays out of the way.
    readonly property bool showsPage: folderModel.pageKind !== ""
    visible: !showsPage
    // Waiting for a server's first answer (a spinner and Stop show instead of the empty folder).
    readonly property bool connecting: folderModel.loading && folderModel.onServer && !folderModel.searching && folderModel.count === 0 && folderModel.errorText.length === 0

    // "details", "icons", "compact", "columns" or "gallery"
    property string viewMode: "details"
    // The size of the icons in the Icons view (Ctrl+scroll, Ctrl+plus, Ctrl+minus;
    // each folder's own).
    property int iconSize: ViewMemory.iconDefault()
    // What the rows are grouped by (FolderModel.GroupNone, GroupName, GroupType,
    // GroupModified); only the Details and Icons views show groups.
    property int groupBy: FolderModel.GroupNone
    // Quick Look is over the window: a player in the gallery stops meanwhile.
    property bool covered: false
    property alias folder: folderModel
    property alias selection: sel
    property alias url: folderModel.url
    // Bumped on every selection change so delegates re-read their state.
    property int selRevision: 0
    property int currentRow: -1
    property int anchorRow: -1
    property string typed: ""
    // The item being renamed in place (empty: none), what has been typed so
    // far (kept if the row scrolls away and back), whether the editor is new
    // (it selects the name without its extension once), and the editor.
    property url renameUrl
    property string renameText: ""
    property bool renameDirty: false
    property var activeEditor: null
    readonly property bool renaming: renameUrl.toString().length > 0
    // Search results are always a Details view (they have a Path column).
    readonly property string shown: folderModel.searching ? "details" : viewMode
    readonly property bool showsDetails: shown === "details"
    readonly property var activeView: {
        switch (shown) {
        case "details":
            return details;
        case "columns":
            return columns;
        case "gallery":
            return gallery;
        default:
            return icons;
        }
    }
    // The FileActions that drags and drops go to.
    property var actions
    // The tab's SearchController: why a search has no results shows in place of them.
    property var search: null
    // The URLs of the selected rows.
    readonly property var selectedUrls: {
        top.selRevision;
        return folderModel.urlsOf(top.selectedRows());
    }

    signal openRequested(var urls)
    // The items to show a menu for (none: the background) and the point in
    // this view's coordinates.
    signal contextMenuRequested(var urls, real x, real y)
    signal navigateRequested(url target)
    // A folder to open in a new background tab (middle click, Ctrl+Enter).
    signal openInNewTabRequested(url target)
    signal renameRequested()
    // Open File Location on the selected search results (Ctrl+Enter).
    signal openLocationRequested(var urls)
    // Escape while the rows are search results: end the search.
    signal searchCloseRequested()
    // Escape while the folder filter is narrowing the items, or Clear Filter
    // on the empty view: end the filter.
    signal filterCloseRequested()
    // Space: Quick Look for the selected file.
    signal quickLookRequested()
    // In the columns: go to `target` (a folder) with the items selected there,
    // and keep the view (the folder's own remembered view is not brought back).
    signal columnNavigateRequested(url target, var select)

    FolderModel {
        id: folderModel
        groupBy: (top.shown === "details" || top.shown === "icons") ? top.groupBy : FolderModel.GroupNone
        // Settings > View > Show Git status (a folder's own check: only a folder on this computer is asked).
        gitBadges: SettingsLogic.gitBadges
    }

    // What a screen reader says of a row besides its name: the type, the size
    // (folders have none) and the Git state.
    function rowDescription(typeText, sizeText, gitBadge) {
        const parts = [typeText];
        if (sizeText.length > 0) {
            parts.push(sizeText);
        }
        const git = [ "", qsTr("Git: modified"), qsTr("Git: new"), qsTr("Git: ignored"), qsTr("Git: conflict") ][gitBadge] ?? "";
        if (git.length > 0) {
            parts.push(git);
        }
        return parts.join(", ");
    }

    // The folder's items, as a list for a screen reader.
    Accessible.role: Accessible.List
    Accessible.name: qsTr("Files")

    // ---- What the folder remembers of its view ----
    // True while the remembered view is being put back (nothing is remembered then).
    property bool restoring: false
    // A move between the columns keeps the view.
    property bool keepView: false

    function currentPrefs() {
        return {
            "mode": viewMode,
            "sort": folderModel.sortColumn,
            "descending": folderModel.sortDescending,
            "icon": iconSize,
            "group": groupBy
        };
    }
    // Shows the folder the way it remembers.
    function applyRemembered() {
        if (keepView || folderModel.searching) {
            return;
        }
        const p = ViewMemory.prefsFor(folderModel.url);
        // In the columns, a folder with no view of its own (one reached by Back,
        // Forward or a click on the sidebar) is shown in the columns too.
        if (!p.remembered && viewMode === "columns" && !ViewMemory.sameForAll) {
            return;
        }
        restoring = true;
        viewMode = p.mode;
        iconSize = p.icon;
        groupBy = p.group;
        // The Trash's columns (7 and 8) only exist in the Trash.
        folderModel.sortColumn = p.sort >= FolderModel.OriginalLocation && !folderModel.trashTop ? FolderModel.Name : p.sort;
        folderModel.sortDescending = p.descending;
        restoring = false;
    }
    // The view was changed: the folder keeps it (only when it differs from what it has).
    function remember() {
        if (restoring || folderModel.searching || !(folderModel.url.toString().length > 0)) {
            return;
        }
        const now = currentPrefs();
        const had = ViewMemory.prefsFor(folderModel.url);
        if (had.mode === now.mode && had.sort === now.sort && had.descending === now.descending && had.icon === now.icon && had.group === now.group) {
            return;
        }
        ViewMemory.remember(folderModel.url, now.mode, now.sort, now.descending, now.icon, now.group);
    }
    // "Reset This Folder's View": back to the shared view.
    function resetRemembered() {
        ViewMemory.forget(folderModel.url);
        applyRemembered();
    }
    // Sort by `column`, ascending or descending (the Sort menu, the column headers).
    function sortBy(column, descending) {
        folderModel.sortColumn = column;
        folderModel.sortDescending = descending;
    }
    onViewModeChanged: remember()
    onIconSizeChanged: remember()
    onGroupByChanged: remember()
    Connections {
        target: folderModel
        function onUrlChanged() {
            top.applyRemembered();
            // The rows are different ones now: a folder row marked as the drop target is not any more.
            top.dropRow = -1;
        }
        function onSortChanged() {
            top.remember();
        }
    }

    ItemSelectionModel {
        id: sel
        model: folderModel
        onSelectionChanged: top.selRevision++
        onCurrentChanged: top.currentRow = sel.currentIndex.row
    }

    Connections {
        target: folderModel
        function onModelReset() {
            top.currentRow = -1;
            top.anchorRow = -1;
            top.selRevision++;
        }
        function onLayoutChanged() {
            top.currentRow = sel.currentIndex.row;
        }
    }

    Timer {
        id: typeReset
        interval: 1000
        onTriggered: top.typed = ""
    }

    // Whether `row` is selected; `revision` only makes bindings re-read it.
    function isSelected(row, revision) {
        return sel.isSelected(folderModel.index(row, 0));
    }

    function selectedRows() {
        const out = [];
        const list = sel.selectedIndexes;
        for (let i = 0; i < list.length; ++i) {
            out.push(list[i].row);
        }
        return out;
    }

    // Selects the listed items (the ones that are shown) and scrolls to the last.
    function selectUrls(urls) {
        sel.clearSelection();
        let last = -1;
        for (const u of urls) {
            const r = folderModel.rowOfUrl(u);
            if (r >= 0) {
                // An item that was put in a folded group opens it: nothing is selected out of sight.
                if (folderModel.isRowCollapsed(r)) {
                    folderModel.toggleGroup(folderModel.groupAt(r));
                }
                sel.select(folderModel.index(r, 0), ItemSelectionModel.Select);
                last = r;
            }
        }
        if (last >= 0) {
            anchorRow = last;
            setCurrent(last);
        }
    }

    // ---- Renaming in place ----
    // F2, Rename: the name of `url` is edited where it is shown. Where it
    // can't be (the gallery, search results, which show no name to edit in
    // the folder), Files asks in a dialog.
    function startRename(url) {
        if (!actions) {
            return;
        }
        const row = folderModel.rowOfUrl(url);
        if (row < 0) {
            return;
        }
        if (shown === "gallery" || folderModel.searching) {
            actions.renameWithDialog(url);
            return;
        }
        // An item in a folded group opens it: nothing is edited out of sight.
        if (folderModel.isRowCollapsed(row)) {
            folderModel.toggleGroup(folderModel.groupAt(row));
        }
        selectUrls([url]);
        renameText = "";
        renameDirty = false;
        renameUrl = url;
    }
    // Something else was clicked: the edit ends the way a click away ends it
    // (renamed when the name can be used). The editor keeps the keyboard until
    // it is gone, so this can't wait for the focus to move.
    function leaveRename() {
        if (renaming && activeEditor) {
            activeEditor.leave();
        }
    }
    // The edit is over (renamed, cancelled or abandoned); the view has the keyboard back.
    function endRename(refocus = true) {
        renameUrl = "";
        renameText = "";
        renameDirty = false;
        renameWanted = "";
        if (refocus) {
            forceActiveFocus();
        }
    }
    // A new folder or file: edited as soon as it is listed.
    property url renameWanted
    function renameWhenListed(url) {
        renameWanted = url;
        renameWantedTimer.restart();
        tryRenameWanted();
    }
    function tryRenameWanted() {
        if (renameWanted.toString().length === 0 || folderModel.rowOfUrl(renameWanted) < 0) {
            return;
        }
        // The rows are still settling (the new item was just added, and the
        // sort moves it): the edit starts when they have.
        renameSettle.restart();
    }
    Timer {
        id: renameSettle
        interval: 120
        onTriggered: {
            const url = top.renameWanted;
            if (url.toString().length > 0 && folderModel.rowOfUrl(url) >= 0) {
                top.renameWanted = "";
                renameWantedTimer.stop();
                top.startRename(url);
            }
        }
    }
    Timer {
        id: renameWantedTimer
        interval: 4000
        onTriggered: top.renameWanted = ""
    }
    Connections {
        target: folderModel
        function onCountChanged() {
            top.tryRenameWanted();
            top.checkRenameStillThere();
        }
        function onLayoutChanged() {
            top.tryRenameWanted();
            top.checkRenameStillThere();
        }
        function onLoadingChanged() {
            top.tryRenameWanted();
        }
        // The folder was left: the edit is over.
        function onUrlChanged() {
            if (top.renaming) {
                top.endRename();
            }
        }
    }
    // The item went away (deleted elsewhere): there is nothing to edit.
    function checkRenameStillThere() {
        if (renaming && !folderModel.loading && folderModel.rowOfUrl(renameUrl) < 0) {
            endRename();
        }
    }

    // A press on a row. Pressing a selected row with no modifier keeps the
    // selection, so a drag can take all of it; the release then narrows it.
    function pressRow(row, mods) {
        leaveRename();
        forceActiveFocus();
        selectFirst = false;
        if (mods === 0 && isSelected(row, selRevision)) {
            setCurrent(row);
            return false;
        }
        chooseRow(row, mods, false);
        return true;
    }

    // A right click on a row at (x, y) of this view: the menu is for the
    // selection, which first becomes this row when it wasn't part of it.
    function rowMenu(row, x, y) {
        leaveRename();
        forceActiveFocus();
        if (!isSelected(row, selRevision)) {
            chooseRow(row, 0, false);
        }
        contextMenuRequested(selectedUrls, x, y);
    }

    // The Menu key or Shift+F10: the menu of the selection at the row the
    // keyboard is on (the item with the cursor when nothing is selected), else
    // of the folder's background near the top left.
    function keyboardMenu() {
        if (selectedRows().length === 0 && currentRow >= 0 && currentRow < folderModel.count) {
            chooseRow(currentRow, 0, false);
        }
        const rows = selectedRows();
        if (rows.length === 0) {
            contextMenuRequested([], Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 3);
            return;
        }
        const row = rows.indexOf(currentRow) >= 0 ? currentRow : Math.min(...rows);
        activeView.reveal(row);
        const r = activeView.rowRect(row);
        contextMenuRequested(selectedUrls, r.x + Math.min(r.width / 2, Kirigami.Units.gridUnit * 5), r.y + r.height);
    }

    function beginDrag() {
        if (actions) {
            actions.startDrag(selectedUrls);
        }
    }

    function setCurrent(row) {
        sel.setCurrentIndex(folderModel.index(row, 0), ItemSelectionModel.NoUpdate);
        top.currentRow = row;
        activeView.reveal(row);
    }

    // A click or a key move onto `row` with the modifiers held.
    function chooseRow(row, mods, keepSelection) {
        if (row < 0 || row >= folderModel.count) {
            return;
        }
        const idx = folderModel.index(row, 0);
        if ((mods & Qt.ShiftModifier) && anchorRow >= 0) {
            sel.select(folderModel.rangeSelection(anchorRow, row), (mods & Qt.ControlModifier) ? ItemSelectionModel.Select : ItemSelectionModel.ClearAndSelect);
        } else if (mods & Qt.ControlModifier) {
            if (!keepSelection) {
                sel.select(idx, ItemSelectionModel.Toggle);
            }
            anchorRow = row;
        } else {
            sel.select(idx, ItemSelectionModel.ClearAndSelect);
            anchorRow = row;
        }
        setCurrent(row);
    }

    function activateRow(row) {
        if (row < 0) {
            return;
        }
        if (folderModel.isDirAt(row)) {
            if (shown === "columns") {
                enterColumn(folderModel.urlAt(row));
            } else {
                navigateRequested(folderModel.urlAt(row));
            }
        } else {
            openRequested([folderModel.urlAt(row)]);
        }
    }

    // ---- Columns ----
    // Goes to `target` and selects `select` there, keeping the view (a step
    // between the columns is not a move to another folder's own view).
    function columnNavigate(target, select) {
        leaveRename();
        keepView = true;
        columnNavigateRequested(target, select);
        keepView = false;
    }
    // Right arrow, Enter or a double click on a folder: its column becomes
    // the one with the keyboard, with its first item selected.
    function enterColumn(target) {
        selectFirst = true;
        selectFirstTimer.restart();
        columnNavigate(target, []);
    }
    // Left arrow: back to the folder this one is in, with this one selected.
    function leaveColumn() {
        const here = folderModel.url;
        const up = StandardPlaces.parentUrl(here);
        if (up.toString() === here.toString()) {
            return;
        }
        columnNavigate(up, [here]);
    }
    // A click on an item in another column: that column's folder is shown
    // with the item selected.
    function pickInColumn(folder, item) {
        columnNavigate(folder, [item]);
    }
    // A right click on an item in another column: its menu is shown once its
    // folder is the tab's and the item is listed.
    property var pendingMenu: null
    function menuAfterLoad(folder, item, x, y) {
        pendingMenu = {
            "folder": folder,
            "item": item,
            "x": x,
            "y": y
        };
        pendingMenuTimer.restart();
    }
    function showPendingMenu() {
        const m = pendingMenu;
        if (!m || folderModel.loading || folderModel.url.toString().replace(/\/+$/, "") !== m.folder.toString().replace(/\/+$/, "")) {
            return;
        }
        const row = folderModel.rowOfUrl(m.item);
        if (row >= 0) {
            pendingMenu = null;
            pendingMenuTimer.stop();
            rowMenu(row, m.x, m.y);
        }
    }
    Timer {
        id: pendingMenuTimer
        interval: 2000
        onTriggered: top.pendingMenu = null
    }
    // Entering a column selects its first item once it is listed (and sorted).
    property bool selectFirst: false
    function selectFirstNow() {
        if (selectFirst && folderModel.count > 0 && !folderModel.loading && selectedRows().length <= 1) {
            const row = folderModel.visibleRowFrom(0, 1);
            if (row >= 0) {
                chooseRow(row, 0, false);
            }
        }
    }
    Timer {
        id: selectFirstTimer
        interval: 1200
        onTriggered: top.selectFirst = false
    }
    Connections {
        target: folderModel
        function onLoadingChanged() {
            top.selectFirstNow();
            top.showPendingMenu();
        }
        function onLayoutChanged() {
            top.selectFirstNow();
            top.showPendingMenu();
        }
    }
    // Going up: in the columns it is Left.
    function goUp() {
        if (shown === "columns") {
            leaveColumn();
        } else {
            navigateRequested(StandardPlaces.parentUrl(folderModel.url));
        }
    }

    // ---- Groups ----
    // A group's header was clicked: its items are hidden (and deselected) or shown again.
    function toggleGroup(label) {
        if (!folderModel.isGroupCollapsed(label)) {
            const r = folderModel.groupRange(label);
            if (r.length === 2) {
                sel.select(folderModel.rangeSelection(r[0], r[1]), ItemSelectionModel.Deselect);
            }
        }
        folderModel.toggleGroup(label);
        if (folderModel.isRowCollapsed(currentRow)) {
            let to = folderModel.visibleRowFrom(currentRow, 1);
            if (to < 0) {
                to = folderModel.visibleRowFrom(currentRow, -1);
            }
            if (to >= 0) {
                setCurrent(to);
            }
        }
    }
    // The row the keyboard reaches from `from` going to `target`: not in a
    // collapsed group (else the nearest in that direction, else where it was).
    function skipCollapsed(from, target) {
        if (!folderModel.grouped) {
            return target;
        }
        const n = folderModel.count;
        const t = Math.max(0, Math.min(n - 1, target));
        const dir = t >= from ? 1 : -1;
        let v = folderModel.visibleRowFrom(t, dir);
        if (v < 0) {
            v = folderModel.visibleRowFrom(t, -dir);
        }
        return v < 0 ? from : v;
    }

    // A middle click on a row: a folder opens in a new background tab.
    function middleRow(row) {
        leaveRename();
        if (row >= 0 && folderModel.isDirAt(row)) {
            openInNewTabRequested(folderModel.urlAt(row));
        }
    }

    // Enter on the selection. With Ctrl held its folders open in new tabs
    // (in the order shown) and the rest opens as usual.
    function activateSelection(inNewTab) {
        const rows = selectedRows();
        if (rows.length === 0) {
            return;
        }
        if (inNewTab) {
            rows.sort((a, b) => a - b);
            const files = [];
            for (const r of rows) {
                if (folderModel.isDirAt(r)) {
                    openInNewTabRequested(folderModel.urlAt(r));
                } else {
                    files.push(folderModel.urlAt(r));
                }
            }
            if (files.length > 0) {
                openRequested(files);
            }
            return;
        }
        if (rows.length === 1) {
            activateRow(rows[0]);
        } else {
            openRequested(folderModel.urlsOf(rows));
        }
    }

    Keys.onPressed: event => {
        // The user took over: the first item is not selected for them any more.
        selectFirst = false;
        const n = folderModel.count;
        const mods = event.modifiers;
        const cur = currentRow < 0 ? 0 : currentRow;
        let target = -2;
        switch (event.key) {
        case Qt.Key_Up:
            if (mods & Qt.AltModifier) {
                goUp();
                event.accepted = true;
                return;
            }
            target = activeView.neighbor(cur, "up");
            break;
        case Qt.Key_Down:
            target = activeView.neighbor(cur, "down");
            break;
        case Qt.Key_Left:
        case Qt.Key_Right:
            // Alt+Left and Alt+Right are Back and Forward (the window's).
            if (mods & Qt.AltModifier) {
                return;
            }
            // In the columns Right goes into a folder and Left back out of it.
            if (shown === "columns" && !(mods & (Qt.ControlModifier | Qt.ShiftModifier))) {
                if (event.key === Qt.Key_Right) {
                    if (cur >= 0 && currentRow >= 0 && folderModel.isDirAt(currentRow)) {
                        enterColumn(folderModel.urlAt(currentRow));
                    }
                } else {
                    leaveColumn();
                }
                event.accepted = true;
                return;
            }
            target = activeView.neighbor(cur, event.key === Qt.Key_Left ? "left" : "right");
            break;
        case Qt.Key_PageUp:
            target = activeView.neighbor(cur, "pageUp");
            break;
        case Qt.Key_PageDown:
            target = activeView.neighbor(cur, "pageDown");
            break;
        case Qt.Key_Home:
            target = folderModel.grouped ? Math.max(0, folderModel.visibleRowFrom(0, 1)) : 0;
            break;
        case Qt.Key_End:
            target = folderModel.grouped ? Math.max(0, folderModel.visibleRowFrom(n - 1, -1)) : n - 1;
            break;
        case Qt.Key_Return:
        case Qt.Key_Enter:
            if ((mods & Qt.ControlModifier) && folderModel.searching) {
                // Ctrl+Enter on a result: its folder, with it selected.
                if (selectedUrls.length > 0) {
                    openLocationRequested(selectedUrls);
                }
            } else {
                activateSelection(!!(mods & Qt.ControlModifier));
            }
            event.accepted = true;
            return;
        case Qt.Key_Space:
            // Space is Quick Look, unless it is part of a name being typed.
            if (typed.length === 0 && (mods & ~Qt.KeypadModifier) === 0) {
                // A key held down opens it once.
                if (!event.isAutoRepeat) {
                    quickLookRequested();
                }
                event.accepted = true;
                return;
            }
            break;
        case Qt.Key_Backspace:
            // Among search results it is not "go up": the folder is not what is shown.
            if (!folderModel.searching) {
                goUp();
            }
            event.accepted = true;
            return;
        case Qt.Key_F2:
            renameRequested();
            event.accepted = true;
            return;
        case Qt.Key_Menu:
            keyboardMenu();
            event.accepted = true;
            return;
        case Qt.Key_F10:
            if (mods & Qt.ShiftModifier) {
                keyboardMenu();
                event.accepted = true;
                return;
            }
            break;
        case Qt.Key_Escape:
            if (folderModel.searching) {
                searchCloseRequested();
            } else if (folderModel.filterActive || folderModel.filterError.length > 0) {
                filterCloseRequested();
            } else {
                sel.clearSelection();
            }
            event.accepted = true;
            return;
        case Qt.Key_A:
            if (mods & Qt.ControlModifier) {
                if (n > 0) {
                    sel.select(folderModel.rangeSelection(0, n - 1), ItemSelectionModel.Select);
                }
                event.accepted = true;
                return;
            }
            break;
        }
        if (target !== -2) {
            if (n > 0) {
                const t = skipCollapsed(cur, Math.max(0, Math.min(n - 1, target)));
                if ((mods & Qt.ControlModifier) && !(mods & Qt.ShiftModifier)) {
                    setCurrent(t);
                } else {
                    chooseRow(t, mods, false);
                }
            }
            event.accepted = true;
            return;
        }
        // Type-ahead: printable text jumps to the first display name starting with it.
        if (event.text.length === 1 && !(mods & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) && event.text >= " " && (typed.length > 0 || event.text !== " ")) {
            typed += event.text;
            typeReset.restart();
            const hit = folderModel.findPrefix(typed, typed.length === 1 ? cur + 1 : cur);
            if (hit >= 0) {
                chooseRow(hit, 0, false);
            }
            event.accepted = true;
        }
    }

    // A click anywhere in the view outside the name being edited ends the edit.
    TapHandler {
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        enabled: top.renaming
        onTapped: point => {
            const e = top.activeEditor;
            if (!e) {
                return;
            }
            const at = e.mapFromItem(top, point.position.x, point.position.y);
            if (at.x < 0 || at.y < 0 || at.x >= e.width || at.y >= e.height) {
                top.leaveRename();
            }
        }
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: (point, button) => {
            // A row's own handler shows its menu.
            if (top.activeView.rowAt(point.position.x, point.position.y) >= 0) {
                return;
            }
            // A column that isn't the tab's has its own menus.
            if (top.shown === "columns" && !columns.inPrimary(point.position.x, point.position.y)) {
                return;
            }
            top.forceActiveFocus();
            sel.clearSelection();
            top.contextMenuRequested([], point.position.x, point.position.y);
        }
    }

    // ---- Files dragged over the folder ----
    // The folder row under a drag (-1: none), where it is drawn, and the name
    // that spring loading goes by (a drag held over a folder for a second opens it).
    property int dropRow: -1
    property rect dropRect: Qt.rect(0, 0, 0, 0)
    readonly property string springId: String(top)

    DropArea {
        id: dropArea
        anchors.fill: parent
        // The folder row at a point; among search results only a folder row
        // takes a drop ("here" is not what is shown).
        function folderRowAt(x, y) {
            const row = top.activeView.rowAt(x, y);
            return row >= 0 && folderModel.isDirAt(row) ? row : -1;
        }
        function over(drag) {
            const row = folderRowAt(drag.x, drag.y);
            if (row !== top.dropRow) {
                top.dropRow = row;
                top.dropRect = row >= 0 ? top.activeView.rowRect(row) : Qt.rect(0, 0, 0, 0);
            }
            if (row < 0) {
                DragWatch.springClear();
                return;
            }
            const target = folderModel.urlAt(row);
            // Not into a folder that is itself being dragged.
            for (const u of drag.urls) {
                if (u.toString().replace(/\/+$/, "") === target.toString().replace(/\/+$/, "")) {
                    DragWatch.springClear();
                    return;
                }
            }
            DragWatch.spring(top.springId + ":" + target, () => top.navigateRequested(target));
        }
        function leave() {
            top.dropRow = -1;
            DragWatch.springClear();
        }
        onEntered: drag => over(drag)
        onPositionChanged: drag => over(drag)
        onExited: leave()
        onDropped: drop => {
            leave();
            if (!top.actions || !drop.hasUrls) {
                return;
            }
            const row = folderRowAt(drop.x, drop.y);
            // Among search results only a folder row takes a drop: "here" is not what is shown.
            if (folderModel.searching && row < 0) {
                return;
            }
            const target = row >= 0 ? folderModel.urlAt(row) : folderModel.url;
            drop.accepted = true;
            top.actions.drop(drop.urls, target);
        }
    }

    // Where a drop would land: the folder row under the pointer, else this
    // folder (a frame round the view).
    Rectangle {
        visible: top.dropRow >= 0
        z: 20
        x: top.dropRect.x
        y: top.dropRect.y
        width: top.dropRect.width
        height: top.dropRect.height
        radius: TelamonStyle.radiusSmall
        color: TelamonStyle.alpha(TelamonStyle.accent, 0.18)
        border.width: 2
        border.color: TelamonStyle.accent
    }
    Rectangle {
        visible: dropArea.containsDrag && top.dropRow < 0 && !folderModel.searching
        z: 20
        anchors.fill: parent
        color: "transparent"
        border.width: 2
        border.color: TelamonStyle.accent
        radius: TelamonStyle.radiusSmall
    }

    // Ctrl+plus, Ctrl+minus (steps) and Ctrl+0 (0): the icons' size in the
    // Icons view, the rows' height in the others (the gallery has neither).
    function zoom(steps) {
        if (shown === "gallery") {
            return;
        }
        if (shown === "icons") {
            iconSize = steps === 0 ? ViewMemory.iconDefault() : ViewMemory.iconStep(iconSize, steps);
        } else if (steps === 0) {
            PreviewLogic.resetZoom();
        } else {
            PreviewLogic.zoom(steps);
        }
    }
    function zoomByWheel(delta) {
        if (shown === "gallery") {
            return;
        }
        if (shown === "icons") {
            const steps = ViewMemory.iconWheelSteps(delta);
            if (steps !== 0) {
                iconSize = ViewMemory.iconStep(iconSize, steps);
            }
        } else {
            PreviewLogic.zoomByWheel(delta);
        }
    }

    // Ctrl and the wheel change the size of the icons or of the rows. Any
    // other wheel movement goes on to the view below.
    MouseArea {
        anchors.fill: parent
        z: 10
        acceptedButtons: Qt.NoButton
        onWheel: wheel => {
            if (wheel.modifiers & Qt.ControlModifier) {
                top.zoomByWheel(wheel.angleDelta.y);
                wheel.accepted = true;
            } else {
                wheel.accepted = false;
            }
        }
    }

    DetailsView {
        id: details
        anchors.fill: parent
        visible: top.shown === "details"
        fv: top
    }

    IconsView {
        id: icons
        anchors.fill: parent
        visible: top.shown === "icons" || top.shown === "compact"
        compact: top.shown === "compact"
        fv: top
    }

    ColumnsView {
        id: columns
        anchors.fill: parent
        visible: top.shown === "columns"
        fv: top
    }

    GalleryView {
        id: gallery
        anchors.fill: parent
        visible: top.shown === "gallery"
        fv: top
    }

    // A server that has not answered yet: a spinner, and Stop.
    ColumnLayout {
        anchors.centerIn: parent
        visible: top.connecting
        spacing: Kirigami.Units.largeSpacing
        TelamonSpinner {
            Layout.alignment: Qt.AlignHCenter
            running: top.connecting
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            textFormat: Text.PlainText
            text: qsTr("Connecting to %1…").arg(StandardPlaces.tabTitle(folderModel.url))
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
            Accessible.role: Accessible.StaticText
            Accessible.name: text
        }
        SecondaryButton {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Stop")
            onClicked: folderModel.stop()
        }
    }

    TelamonEmptyState {
        anchors.centerIn: parent
        visible: !top.connecting && (folderModel.searching ? (folderModel.count === 0 && !(top.search && top.search.pending)) : (folderModel.errorText.length > 0 || folderModel.count === 0))
        // The filter holds every item back: the folder is not empty.
        readonly property bool filteredOut: !folderModel.searching && folderModel.filterActive && folderModel.count === 0 && folderModel.filterTotal > 0
        symbol: {
            if (folderModel.searching) {
                return folderModel.loading ? Symbols.HourglassEmpty : Symbols.SearchOff;
            }
            if (filteredOut) {
                return Symbols.SearchOff;
            }
            if (folderModel.unreachable) {
                return Symbols.Lan;
            }
            return folderModel.errorText.length > 0 ? Symbols.FolderOff : (folderModel.loading ? Symbols.HourglassEmpty : (folderModel.stopped ? Symbols.FolderOff : Symbols.FolderOpen));
        }
        title: {
            if (folderModel.searching) {
                if (top.search && top.search.failureTitle.length > 0) {
                    return top.search.failureTitle;
                }
                return folderModel.loading ? qsTr("Searching…") : qsTr("No Results");
            }
            if (filteredOut) {
                return qsTr("Nothing Matches the Filter");
            }
            if (folderModel.unreachable) {
                return qsTr("Can't Reach the Server");
            }
            if (folderModel.errorText.length > 0) {
                return folderModel.inArchive ? qsTr("Can't Open This Archive") : qsTr("Can't Open This Folder");
            }
            return folderModel.loading ? qsTr("Loading…") : (folderModel.stopped ? qsTr("Stopped") : qsTr("This Folder Is Empty"));
        }
        text: {
            if (folderModel.searching) {
                if (top.search && top.search.failureText.length > 0) {
                    return top.search.failureText;
                }
                if (folderModel.loading) {
                    return "";
                }
                return top.search && top.search.chipLevel === 2 && !top.search.live ? qsTr("Nothing matches so far. The search index is still updating, so results may be missing.") : qsTr("Nothing here matches. Try other words, or another scope or filter.");
            }
            if (filteredOut) {
                return qsTr("None of the %1 items here match. Change the filter or clear it.").arg(folderModel.filterTotal);
            }
            if (folderModel.errorText.length === 0 && folderModel.stopped) {
                return qsTr("Listing this folder was stopped.");
            }
            return folderModel.errorText;
        }
        actionText: filteredOut ? qsTr("Clear Filter") : (folderModel.searching ? (top.search && top.search.failureTitle.length > 0 ? qsTr("Try Again") : "") : (folderModel.errorText.length > 0 || folderModel.stopped ? qsTr("Retry") : ""))
        onTriggered: {
            if (filteredOut) {
                top.filterCloseRequested();
            } else {
                folderModel.refresh();
            }
        }
    }

    // A listing from a server under way (or stopped half way): Stop, or Retry.
    Rectangle {
        visible: !top.connecting && !folderModel.searching && folderModel.onServer && folderModel.count > 0 && (folderModel.loading || folderModel.stopped)
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: Kirigami.Units.largeSpacing
        radius: TelamonStyle.radius
        color: Kirigami.Theme.backgroundColor
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.2)
        implicitWidth: pillRow.implicitWidth + Kirigami.Units.gridUnit
        implicitHeight: pillRow.implicitHeight + Kirigami.Units.smallSpacing * 2
        width: implicitWidth
        height: implicitHeight
        RowLayout {
            id: pillRow
            anchors.centerIn: parent
            spacing: Kirigami.Units.smallSpacing
            TelamonSpinner {
                running: folderModel.loading
                visible: folderModel.loading
                implicitWidth: Kirigami.Units.iconSizes.small
                implicitHeight: implicitWidth
            }
            Text {
                textFormat: Text.PlainText
                text: folderModel.loading ? qsTr("Loading…") : qsTr("Stopped. Some items may be missing.")
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
            }
            TextButton {
                text: folderModel.loading ? qsTr("Stop") : qsTr("Retry")
                onClicked: folderModel.loading ? folderModel.stop() : folderModel.refresh()
            }
        }
    }

    // Shown when the folder went away and its parent took its place.
    Rectangle {
        visible: folderModel.notice.length > 0
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottomMargin: Kirigami.Units.largeSpacing
        radius: 6
        color: Kirigami.Theme.backgroundColor
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.2)
        implicitWidth: noticeText.implicitWidth + Kirigami.Units.gridUnit
        implicitHeight: noticeText.implicitHeight + Kirigami.Units.gridUnit
        Text {
            id: noticeText
            anchors.centerIn: parent
            textFormat: Text.PlainText
            text: folderModel.notice
            color: Kirigami.Theme.textColor
        }
    }
}
