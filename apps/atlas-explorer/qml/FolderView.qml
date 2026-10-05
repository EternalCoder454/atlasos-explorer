pragma ComponentBehavior: Bound
import QtQuick
import QtQml.Models
import org.kde.kirigami as Kirigami
import Atlas.Ui

// A folder shown as details, icons or a compact list: selection, keyboard
// navigation and type-ahead live here, the views only draw rows. Opening a
// file is not done here: `openRequested` hands the URLs to whoever owns the
// trust prompts.
FocusScope {
    id: top

    // "details", "icons" or "compact"
    property string viewMode: "details"
    property int iconSize: 96
    property alias folder: folderModel
    property alias selection: sel
    property alias url: folderModel.url
    // Bumped on every selection change so delegates re-read their state.
    property int selRevision: 0
    property int currentRow: -1
    property int anchorRow: -1
    property string typed: ""
    readonly property var activeView: viewMode === "details" ? details : icons

    signal openRequested(var urls)
    signal navigateRequested(url target)

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

    function activateSelection() {
        const rows = selectedRows();
        if (rows.length === 0) {
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
            target = activeView.neighbor(cur, "left");
            break;
        case Qt.Key_Right:
            target = activeView.neighbor(cur, "right");
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
            activateSelection();
            event.accepted = true;
            return;
        case Qt.Key_Backspace:
            navigateRequested(StandardPlaces.parentUrl(folderModel.url));
            event.accepted = true;
            return;
        case Qt.Key_Escape:
            sel.clearSelection();
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

    DetailsView {
        id: details
        anchors.fill: parent
        visible: top.viewMode === "details"
        fv: top
    }

    IconsView {
        id: icons
        anchors.fill: parent
        visible: top.viewMode !== "details"
        compact: top.viewMode === "compact"
        fv: top
    }

    AtlasEmptyState {
        anchors.centerIn: parent
        visible: folderModel.errorText.length > 0 || (folderModel.count === 0)
        symbol: folderModel.errorText.length > 0 ? Symbols.FolderOff : (folderModel.loading ? Symbols.HourglassEmpty : Symbols.FolderOpen)
        title: folderModel.errorText.length > 0 ? qsTr("Can't Open This Folder") : (folderModel.loading ? qsTr("Loading…") : qsTr("This Folder Is Empty"))
        text: folderModel.errorText
        actionText: folderModel.errorText.length > 0 ? qsTr("Retry") : ""
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
