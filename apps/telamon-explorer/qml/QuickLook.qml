pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Quick Look: a large preview of the file selected in a view, over the
// window (Space). Left and Up, Right and Down move to the previous and next
// file: through the selection when it holds two files or more, else through
// the folder (or the search results, or the Trash) with the selection
// following. Enter opens the file in its default app, Space or Escape closes.
// This item keeps the keyboard while it is open; the controls in it never
// take it.
FocusScope {
    id: ql

    // The FolderView that has the files, while open.
    property var fv: null
    readonly property bool opened: visible
    // The file shown (a URL), its row in the view, and what the row shows.
    property url current
    property int row: -1
    property var info: ({})
    // The files stepped through when the selection holds more than one (their
    // URLs as text, in the order of the view); empty: the whole folder.
    property var ring: []
    // Rows went away since the ring was last checked.
    property bool pruneRing: false

    readonly property int position: ring.length > 0 ? ring.indexOf(current.toString()) + 1 : row + 1
    readonly property int total: ring.length > 0 ? ring.length : (fv ? fv.folder.count : 0)

    // Open the file in its app: the view's own open (Run or open? for programs, folders open in the tab).
    signal openFile(int row)

    anchors.fill: parent
    visible: false
    z: 100
    // Drawn as one layer: without it the software renderer paints the icons
    // of the rows behind over the card (FileIcon fixes that for the icons).
    layer.enabled: true
    Accessible.role: Accessible.Dialog
    Accessible.name: qsTr("Quick Look")
    // Which file, and where it is among the others: read when it opens and again at each step.
    Accessible.description: info.name !== undefined ? qsTr("%1, %2 of %3").arg(info.name).arg(position).arg(total) : ""
    onInfoChanged: {
        if (visible && info.name !== undefined) {
            ql.Accessible.announce(qsTr("%1, %2 of %3").arg(info.name).arg(position).arg(total));
        }
    }

    // Shows the file selected in `view`; nothing happens when there is none.
    function openFor(view) {
        if (!view || view.folder.count === 0) {
            return;
        }
        const selected = view.selectedRows().sort((a, b) => a - b);
        let start = -1;
        let urls = [];
        if (selected.length > 1) {
            start = view.currentRow >= 0 && selected.indexOf(view.currentRow) >= 0 ? view.currentRow : selected[0];
            urls = view.folder.urlsOf(selected).map(u => u.toString());
        } else if (selected.length === 1) {
            start = selected[0];
        } else if (view.currentRow >= 0) {
            start = view.currentRow;
        }
        if (start < 0) {
            return;
        }
        fv = view;
        ring = urls;
        row = start;
        current = view.folder.urlAt(start);
        info = view.folder.detailsAt(start);
        visible = true;
        forceActiveFocus();
    }

    function close() {
        if (!visible) {
            return;
        }
        visible = false;
        const view = fv;
        fv = null;
        ring = [];
        row = -1;
        info = ({});
        if (view) {
            view.forceActiveFocus();
        }
    }

    // The view's rows moved (sorted, listed again, a file came or went): find
    // the file again, and close when it is gone.
    function sync() {
        if (!visible || !fv) {
            return;
        }
        const f = fv.folder;
        if (ring.length > 0 && pruneRing) {
            ring = f.filterExisting(ring.map(u => Qt.url(u))).map(u => u.toString());
        }
        pruneRing = false;
        if (row < 0 || row >= f.count || f.urlAt(row).toString() !== current.toString()) {
            row = f.rowOfUrl(current);
        }
        if (row < 0) {
            close();
            return;
        }
        info = f.detailsAt(row);
    }

    // Moves `by` files on (negative: back), stopping at the ends.
    function step(by) {
        if (!visible || !fv) {
            return;
        }
        const f = fv.folder;
        if (ring.length > 0) {
            const i = ring.indexOf(current.toString());
            const to = Math.max(0, Math.min(ring.length - 1, i + by));
            if (to === i) {
                return;
            }
            const r = f.rowOfUrl(Qt.url(ring[to]));
            if (r < 0) {
                sync();
                return;
            }
            show(r);
            // Only the cursor moves: the selection is what is being browsed.
            fv.setCurrent(r);
        } else {
            const to = Math.max(0, Math.min(f.count - 1, row + by));
            if (to === row) {
                return;
            }
            show(to);
            // The selection follows.
            fv.chooseRow(to, 0, false);
        }
    }

    function show(r) {
        row = r;
        current = fv.folder.urlAt(r);
        info = fv.folder.detailsAt(r);
    }

    function openCurrent() {
        const r = row;
        close();
        if (r >= 0) {
            openFile(r);
        }
    }

    Connections {
        target: ql.fv ? ql.fv.folder : null
        function onModelReset() {
            ql.pruneRing = true;
            ql.sync();
        }
        function onLayoutChanged() {
            ql.sync();
        }
        function onRowsInserted() {
            ql.sync();
        }
        function onRowsRemoved() {
            ql.pruneRing = true;
            ql.sync();
        }
        function onDataChanged() {
            ql.sync();
        }
    }

    Keys.onPressed: event => {
        if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) {
            return;
        }
        switch (event.key) {
        case Qt.Key_Space:
            // A key held down since before Quick Look opened must not close it again.
            if (!event.isAutoRepeat) {
                close();
            }
            break;
        case Qt.Key_Escape:
            close();
            break;
        case Qt.Key_Left:
        case Qt.Key_Up:
            step(-1);
            break;
        case Qt.Key_Right:
        case Qt.Key_Down:
            step(1);
            break;
        case Qt.Key_Home:
            step(-1000000);
            break;
        case Qt.Key_End:
            step(1000000);
            break;
        case Qt.Key_Return:
        case Qt.Key_Enter:
            openCurrent();
            break;
        default:
            // Other keys do nothing here, and do not reach the view behind.
            break;
        }
        event.accepted = true;
    }

    // The dimmed window behind: a click on it closes.
    Rectangle {
        anchors.fill: parent
        // A scrim is black in both themes, as TelamonDialog's is. // telamon-lint: allow-raw
        color: Qt.rgba(0, 0, 0, 0.5)
        // Every click and the wheel stop here: nothing under the dimmed window
        // reacts while Quick Look is open. A click closes it.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onClicked: ql.close()
            onWheel: wheel => wheel.accepted = true
        }
    }

    Rectangle {
        id: card
        anchors.centerIn: parent
        width: Math.min(parent.width - Kirigami.Units.gridUnit * 4, Kirigami.Units.gridUnit * 64)
        height: Math.min(parent.height - Kirigami.Units.gridUnit * 4, Kirigami.Units.gridUnit * 44)
        radius: TelamonStyle.radiusLarge
        color: TelamonStyle.floatingBackground
        border.width: 1
        border.color: TelamonStyle.separator
        // A click on the card is not a click on the dimmed window.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            z: -1
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                FileIcon {
                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                    source: ql.info.iconName ?? ""
                }
                Text {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    font.bold: true
                    color: Kirigami.Theme.textColor
                    text: ql.info.name ?? ""
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                Text {
                    visible: ql.total > 1
                    textFormat: Text.PlainText
                    color: TelamonStyle.textMuted
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    text: qsTr("%1 of %2").arg(ql.position).arg(ql.total)
                }
                ToolbarButton {
                    symbol: Symbols.Close
                    text: qsTr("Close")
                    shortcutText: "Space"
                    onClicked: ql.close()
                }
            }

            PreviewBody {
                id: body
                Layout.fillWidth: true
                Layout.fillHeight: true
                info: ql.info
                thumbSide: 1024
                iconSide: Kirigami.Units.iconSizes.huge
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                Text {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: TelamonStyle.textMuted
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    text: {
                        const parts = [body.loader.kindText || (ql.info.typeText ?? ""), ql.info.sizeText ?? "", body.loader.dimensionsText || body.mediaDimensions, body.mediaDuration, ql.info.modifiedText ?? ""];
                        return parts.filter(p => p && p.length > 0).join("  ·  ");
                    }
                }
                Text {
                    textFormat: Text.PlainText
                    color: TelamonStyle.textMuted
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    text: qsTr("Arrows Browse  ·  Enter Open  ·  Space Close")
                }
            }
        }
    }
}
