pragma ComponentBehavior: Bound
import QtQuick
import QtQml.Models
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A folder shown as details, icons or a compact list: selection, keyboard
// navigation and type-ahead live here, the views only draw rows. Opening a
// file is not done here: `openRequested` hands the URLs to whoever owns the
// trust prompts.
FocusScope {
    id: top

    // "details", "icons" or "compact"
    property string viewMode: "details"
    // The size of the icons in the Icons view (Ctrl+scroll, Ctrl+plus, Ctrl+minus; kept).
    readonly property int iconSize: PreviewLogic.iconSize
    property alias folder: folderModel
    property alias selection: sel
    property alias url: folderModel.url
    // Bumped on every selection change so delegates re-read their state.
    property int selRevision: 0
    property int currentRow: -1
    property int anchorRow: -1
    property string typed: ""
    // Search results are always a Details view (they have a Path column).
    readonly property bool showsDetails: viewMode === "details" || folderModel.searching
    readonly property var activeView: showsDetails ? details : icons
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
    signal contextMenuRequested(var urls)
    signal navigateRequested(url target)
    // A folder to open in a new background tab (middle click, Ctrl+Enter).
    signal openInNewTabRequested(url target)
    signal renameRequested()
    // Open File Location on the selected search results (Ctrl+Enter).
    signal openLocationRequested(var urls)
    // Escape while the rows are search results: end the search.
    signal searchCloseRequested()
    // Space: Quick Look for the selected file.
    signal quickLookRequested()

    FolderModel {
        id: folderModel
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
                sel.select(folderModel.index(r, 0), ItemSelectionModel.Select);
                last = r;
            }
        }
        if (last >= 0) {
            anchorRow = last;
            setCurrent(last);
        }
    }

    // A press on a row. Pressing a selected row with no modifier keeps the
    // selection, so a drag can take all of it; the release then narrows it.
    function pressRow(row, mods) {
        forceActiveFocus();
        if (mods === 0 && isSelected(row, selRevision)) {
            setCurrent(row);
            return false;
        }
        chooseRow(row, mods, false);
        return true;
    }

    // A right click on a row: the menu is for the selection, which first
    // becomes this row when it wasn't part of it.
    function rowMenu(row) {
        forceActiveFocus();
        if (!isSelected(row, selRevision)) {
            chooseRow(row, 0, false);
        }
        contextMenuRequested(selectedUrls);
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
            navigateRequested(folderModel.urlAt(row));
        } else {
            openRequested([folderModel.urlAt(row)]);
        }
    }

    // A middle click on a row: a folder opens in a new background tab.
    function middleRow(row) {
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
        const n = folderModel.count;
        const mods = event.modifiers;
        const cur = currentRow < 0 ? 0 : currentRow;
        let target = -2;
        switch (event.key) {
        case Qt.Key_Up:
            if (mods & Qt.AltModifier) {
                navigateRequested(StandardPlaces.parentUrl(folderModel.url));
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
            target = activeView.neighbor(cur, event.key === Qt.Key_Left ? "left" : "right");
            break;
        case Qt.Key_PageUp:
            target = activeView.neighbor(cur, "pageUp");
            break;
        case Qt.Key_PageDown:
            target = activeView.neighbor(cur, "pageDown");
            break;
        case Qt.Key_Home:
            target = 0;
            break;
        case Qt.Key_End:
            target = n - 1;
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
                navigateRequested(StandardPlaces.parentUrl(folderModel.url));
            }
            event.accepted = true;
            return;
        case Qt.Key_F2:
            renameRequested();
            event.accepted = true;
            return;
        case Qt.Key_Escape:
            if (folderModel.searching) {
                searchCloseRequested();
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
                const t = Math.max(0, Math.min(n - 1, target));
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

    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: (point, button) => {
            // A row's own handler shows its menu.
            if (top.activeView.rowAt(point.position.x, point.position.y) >= 0) {
                return;
            }
            top.forceActiveFocus();
            sel.clearSelection();
            top.contextMenuRequested([]);
        }
    }

    DropArea {
        anchors.fill: parent
        onDropped: drop => {
            if (!top.actions || !drop.hasUrls) {
                return;
            }
            const row = top.activeView.rowAt(drop.x, drop.y);
            // Among search results only a folder row takes a drop: "here" is not what is shown.
            if (folderModel.searching && !(row >= 0 && folderModel.isDirAt(row))) {
                return;
            }
            const target = row >= 0 && folderModel.isDirAt(row) ? folderModel.urlAt(row) : folderModel.url;
            drop.accepted = true;
            top.actions.drop(drop.urls, target);
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
                PreviewLogic.zoomByWheel(!top.showsDetails && top.viewMode === "icons", wheel.angleDelta.y);
                wheel.accepted = true;
            } else {
                wheel.accepted = false;
            }
        }
    }

    DetailsView {
        id: details
        anchors.fill: parent
        visible: top.showsDetails
        fv: top
    }

    IconsView {
        id: icons
        anchors.fill: parent
        visible: !top.showsDetails
        compact: top.viewMode === "compact"
        fv: top
    }

    TelamonEmptyState {
        anchors.centerIn: parent
        visible: folderModel.searching ? (folderModel.count === 0 && !(top.search && top.search.pending)) : (folderModel.errorText.length > 0 || folderModel.count === 0)
        symbol: {
            if (folderModel.searching) {
                return folderModel.loading ? Symbols.HourglassEmpty : Symbols.SearchOff;
            }
            return folderModel.errorText.length > 0 ? Symbols.FolderOff : (folderModel.loading ? Symbols.HourglassEmpty : Symbols.FolderOpen);
        }
        title: {
            if (folderModel.searching) {
                if (top.search && top.search.failureTitle.length > 0) {
                    return top.search.failureTitle;
                }
                return folderModel.loading ? qsTr("Searching…") : qsTr("No Results");
            }
            return folderModel.errorText.length > 0 ? qsTr("Can't Open This Folder") : (folderModel.loading ? qsTr("Loading…") : qsTr("This Folder Is Empty"));
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
            return folderModel.errorText;
        }
        actionText: folderModel.searching ? (top.search && top.search.failureTitle.length > 0 ? qsTr("Try Again") : "") : (folderModel.errorText.length > 0 ? qsTr("Retry") : "")
        onTriggered: folderModel.refresh()
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
