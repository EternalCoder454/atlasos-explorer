pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The Details view: one fixed-height row per item, a header with sortable and
// resizable columns. Only the rows on screen exist.
Item {
    id: root

    required property var fv
    // Ctrl+scroll, Ctrl+plus and Ctrl+minus change the rows' height (kept); the header stays.
    readonly property int rowHeight: PreviewLogic.rowHeight
    readonly property int headerHeight: Kirigami.Units.gridUnit * 2
    readonly property int iconSide: Math.min(rowHeight - 4, Math.max(Kirigami.Units.iconSizes.smallMedium, Math.round(rowHeight * 0.62)))
    // Search results have a Path column (the folder each one is in) after the name.
    readonly property bool withPath: fv.folder.searching
    // Results of a search inside files also show the matching line (a Match column after Path).
    readonly property bool withMatch: fv.folder.searching && fv.folder.hasSnippets
    // The top of the Trash: the folder each item was in, and when it was deleted.
    readonly property bool withTrash: fv.folder.trashTop
    // Widths of the columns by key. The Trash has an Original Location and a
    // Date Deleted, so its columns are narrower to fit the window.
    function defaultWidths(trash) {
        const gu = Kirigami.Units.gridUnit;
        return trash ? {
            "name": gu * 15,
            "path": gu * 13,
            "match": gu * 24,
            "size": gu * 5.5,
            "type": gu * 10,
            "modified": gu * 10,
            "tags": gu * 10,
            "dimensions": gu * 8,
            "duration": gu * 6,
            "taken": gu * 10
        } : {
            "name": gu * 22,
            "path": gu * 16,
            "match": gu * 26,
            "size": gu * 7,
            "type": gu * 12,
            "modified": gu * 11,
            "tags": gu * 11,
            "dimensions": gu * 8,
            "duration": gu * 6,
            "taken": gu * 11
        };
    }
    property var widths: defaultWidths(withTrash)
    // The Trash has its own layout: a drag in another folder does not carry over.
    onWithTrashChanged: widths = defaultWidths(withTrash)
    readonly property var baseColumns: withTrash ? [
        { key: "name", title: qsTr("Name"), sort: FolderModel.Name },
        { key: "path", title: qsTr("Original Location"), sort: FolderModel.OriginalLocation },
        { key: "size", title: qsTr("Size"), sort: FolderModel.Size },
        { key: "type", title: qsTr("Type"), sort: FolderModel.Type },
        { key: "modified", title: qsTr("Date Deleted"), sort: FolderModel.DateDeleted }
    ] : withPath ? [
        { key: "name", title: qsTr("Name"), sort: FolderModel.Name },
        { key: "path", title: qsTr("Path"), sort: -1 },
        { key: "size", title: qsTr("Size"), sort: FolderModel.Size },
        { key: "type", title: qsTr("Type"), sort: FolderModel.Type },
        { key: "modified", title: qsTr("Modified"), sort: FolderModel.Modified }
    ] : [
        { key: "name", title: qsTr("Name"), sort: FolderModel.Name },
        { key: "size", title: qsTr("Size"), sort: FolderModel.Size },
        { key: "type", title: qsTr("Type"), sort: FolderModel.Type },
        { key: "modified", title: qsTr("Modified"), sort: FolderModel.Modified }
    ]
    readonly property var columns: {
        const list = baseColumns.slice();
        if (withMatch) {
            list.splice(2, 0, { key: "match", title: qsTr("Match"), sort: -1 });
        }
        if (withTags) {
            list.push({ key: "tags", title: qsTr("Tags"), sort: -1 });
        }
        if (withDimensions) {
            list.push({ key: "dimensions", title: qsTr("Dimensions"), sort: -1 });
        }
        if (withDuration) {
            list.push({ key: "duration", title: qsTr("Duration"), sort: -1 });
        }
        if (withTaken) {
            list.push({ key: "taken", title: qsTr("Date Taken"), sort: -1 });
        }
        return list;
    }
    readonly property real pathWidth: withPath || withTrash ? widths.path : 0
    readonly property real matchWidth: withMatch ? widths.match : 0
    // The optional columns (View menu): Tags, then the ones read from the files.
    // None in the Trash, where an item's tags don't matter.
    readonly property bool withTags: ColumnLogic.tags && !withTrash
    readonly property bool withDimensions: ColumnLogic.dimensions && !withTrash
    readonly property bool withDuration: ColumnLogic.duration && !withTrash
    readonly property bool withTaken: ColumnLogic.taken && !withTrash
    readonly property real extraWidth: (withTags ? widths.tags : 0) + (withDimensions ? widths.dimensions : 0) + (withDuration ? widths.duration : 0) + (withTaken ? widths.taken : 0)
    readonly property real totalWidth: widths.name + pathWidth + matchWidth + widths.size + widths.type + widths.modified + extraWidth
    // Only while a column wants them are the files read for their details.
    Binding {
        target: root.fv.folder
        property: "wantMeta"
        value: root.visible && (root.withDimensions || root.withDuration || root.withTaken)
    }
    readonly property int pageRows: Math.max(1, Math.floor(list.height / rowHeight) - 1)

    function reveal(row) {
        list.positionViewAtIndex(row, ListView.Contain);
    }

    // The row at a point in this item's coordinates, -1 for none (a group's
    // header is above the row it comes before, and is not a row).
    function rowAt(x, y) {
        const cy = y + list.contentY;
        const index = list.indexAt(x + list.contentX, cy);
        if (index < 0) {
            return -1;
        }
        const item = list.itemAtIndex(index);
        if (item && (cy < item.y || cy >= item.y + item.height)) {
            return -1;
        }
        return index;
    }

    // Where a row is, in the folder view's coordinates (the row may be off screen).
    function rowRect(row) {
        const item = list.itemAtIndex(row);
        if (item) {
            const at = item.mapToItem(root.fv, 0, 0);
            return Qt.rect(at.x, at.y, list.width, rowHeight);
        }
        // Not made yet: worked out from where the list is.
        const at = list.mapToItem(root.fv, 0, row * rowHeight - list.contentY);
        return Qt.rect(at.x, at.y, list.width, rowHeight);
    }

    function neighbor(row, dir) {
        switch (dir) {
        case "up":
        case "left":
            return row - 1;
        case "down":
        case "right":
            return row + 1;
        case "pageUp":
            return row - pageRows;
        default:
            return row + pageRows;
        }
    }

    ListView {
        id: list
        anchors.fill: parent
        model: root.visible ? root.fv.folder : null
        reuseItems: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        contentWidth: root.totalWidth
        flickableDirection: Flickable.HorizontalAndVerticalFlick
        headerPositioning: ListView.OverlayHeader
        cacheBuffer: root.rowHeight * 8
        QQC2.ScrollBar.vertical: TelamonScrollBar {}
        QQC2.ScrollBar.horizontal: TelamonScrollBar {}

        // Group by: the rows of a group are together, each group starts with a header.
        // (Not grouped, every row's group is empty and the header takes no room. The
        // property stays as it is: changing it while rows are shown crashes the list.)
        section.property: "groupKey"
        section.criteria: ViewSection.FullString
        section.delegate: GroupHeader {
            required property string section
            width: Math.max(list.width, root.totalWidth)
            fv: root.fv
            label: section
            // Read again when the groups are worked out again.
            count: {
                root.fv.folder.groupRevision;
                return root.fv.folder.groupCount(section);
            }
            collapsed: {
                root.fv.folder.groupRevision;
                return root.fv.folder.isGroupCollapsed(section);
            }
            visible: section.length > 0
            height: visible ? implicitHeight : 0
        }

        header: Rectangle {
            z: 2
            width: Math.max(list.width, root.totalWidth)
            height: root.headerHeight
            color: Kirigami.Theme.backgroundColor
            Row {
                Repeater {
                    model: root.columns
                    Item {
                        id: cell
                        required property var modelData
                        required property int index
                        width: root.widths[modelData.key]
                        height: root.headerHeight
                        readonly property bool sorted: root.fv.folder.sortColumn === modelData.sort
                        // Clicking a header sorts by it, then turns the order round.
                        function sortBy() {
                            const f = root.fv.folder;
                            if (cell.modelData.sort < 0) {
                                return;
                            }
                            if (f.sortColumn === cell.modelData.sort) {
                                f.sortDescending = !f.sortDescending;
                            } else {
                                f.sortColumn = cell.modelData.sort;
                                f.sortDescending = false;
                            }
                        }
                        // A column header, sorted or not, that a screen reader can press (the Sort menu is the keyboard's way).
                        Accessible.role: Accessible.ColumnHeader
                        Accessible.name: cell.modelData.title
                        Accessible.description: cell.sorted ? (root.fv.folder.sortDescending ? qsTr("Sorted, descending") : qsTr("Sorted, ascending")) : ""
                        Accessible.onPressAction: cell.sortBy()
                        Text {
                            Accessible.ignored: true
                            anchors.fill: parent
                            anchors.leftMargin: Kirigami.Units.largeSpacing
                            anchors.rightMargin: Kirigami.Units.gridUnit * 1.5
                            verticalAlignment: Text.AlignVCenter
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            text: cell.modelData.title + (cell.sorted ? (root.fv.folder.sortDescending ? "  ▼" : "  ▲") : "")
                            color: Kirigami.Theme.textColor
                            font.bold: cell.sorted
                        }
                        MouseArea {
                            anchors.fill: parent
                            enabled: cell.modelData.sort >= 0
                            onClicked: cell.sortBy()
                        }
                        Rectangle {
                            anchors.right: parent.right
                            width: 1
                            height: parent.height
                            color: Qt.alpha(Kirigami.Theme.textColor, TelamonStyle.highContrast ? 0.5 : 0.15)
                        }
                        MouseArea {
                            anchors.right: parent.right
                            width: Kirigami.Units.largeSpacing
                            height: parent.height
                            cursorShape: Qt.SizeHorCursor
                            property real startX
                            property real startWidth
                            onPressed: mouse => {
                                startX = mapToItem(null, mouse.x, 0).x;
                                startWidth = root.widths[cell.modelData.key];
                            }
                            onPositionChanged: mouse => {
                                if (!pressed) {
                                    return;
                                }
                                const w = Object.assign({}, root.widths);
                                w[cell.modelData.key] = Math.max(Kirigami.Units.gridUnit * 4, startWidth + mapToItem(null, mouse.x, 0).x - startX);
                                root.widths = w;
                            }
                        }
                    }
                }
            }
        }

        delegate: Item {
            id: row
            required property int index
            required property url url
            required property string name
            required property string iconName
            required property bool isDir
            required property bool isHidden
            required property bool isCut
            required property string sizeText
            required property string modifiedText
            required property string typeText
            required property string pathText
            required property string snippetText
            required property string originText
            required property string deletedText
            required property var tagColours
            required property string tagsText
            required property string dimensionsText
            required property string durationText
            required property string takenText
            required property bool groupCollapsed
            required property int gitBadge
            width: Math.max(list.width, root.totalWidth)
            // The rows of a collapsed group take no room.
            height: groupCollapsed ? 0 : root.rowHeight
            visible: !groupCollapsed
            readonly property bool selected: root.fv.isSelected(index, root.fv.selRevision)
            readonly property bool current: root.fv.currentRow === index
            // Text colours: under high contrast a selected row is solid and its text goes with it.
            readonly property color ink: TelamonStyle.highContrast && selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
            readonly property color inkMuted: TelamonStyle.highContrast ? ink : Qt.alpha(Kirigami.Theme.textColor, 0.75)
            // The name is being edited where it is shown.
            readonly property bool editing: root.fv.renaming && root.fv.renameUrl.toString() === url.toString()

            // What a screen reader says: the name, then the type, size and Git state, and whether the row is selected.
            Accessible.role: Accessible.ListItem
            Accessible.name: row.name
            Accessible.description: root.fv.rowDescription(row.typeText, row.sizeText, row.gitBadge)
            Accessible.selectable: true
            Accessible.selected: row.selected
            Accessible.focusable: true
            Accessible.focused: row.current && root.fv.activeFocus

            Rectangle {
                anchors.fill: parent
                anchors.margins: 1
                radius: TelamonStyle.radiusSmall
                color: row.selected ? (TelamonStyle.highContrast ? Kirigami.Theme.highlightColor : Qt.alpha(Kirigami.Theme.highlightColor, 0.35)) : (mouse.containsMouse ? Qt.alpha(Kirigami.Theme.textColor, 0.07) : "transparent")
                border.width: row.current && root.fv.activeFocus ? (TelamonStyle.highContrast ? 3 : 2) : 0
                border.color: TelamonStyle.focus
            }
            Row {
                opacity: (row.isHidden ? 0.6 : 1) * (row.isCut ? 0.5 : 1)
                Item {
                    width: root.widths.name
                    height: root.rowHeight
                    Kirigami.Icon {
                        id: icon
                        // An anchor, not an x: it follows a right-to-left layout.
                        anchors.left: parent.left
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        width: root.iconSide
                        height: width
                        source: row.iconName
                    }
                    GitBadge {
                        id: git
                        anchors.right: parent.right
                        anchors.rightMargin: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        code: row.gitBadge
                        dot: Math.round(root.iconSide * 0.8)
                    }
                    TagDots {
                        id: dots
                        anchors.right: git.visible ? git.left : parent.right
                        anchors.rightMargin: git.visible ? Kirigami.Units.smallSpacing : Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        colours: row.tagColours
                        dot: Math.round(root.iconSide * 0.5)
                    }
                    Text {
                        anchors.left: icon.right
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.right: parent.right
                        anchors.rightMargin: Kirigami.Units.largeSpacing + (dots.visible ? dots.width + Kirigami.Units.smallSpacing : 0) + (git.visible ? git.width + Kirigami.Units.smallSpacing : 0)
                        height: parent.height
                        visible: !row.editing
                        verticalAlignment: Text.AlignVCenter
                        textFormat: Text.PlainText
                        elide: Text.ElideMiddle
                        horizontalAlignment: Text.AlignLeft
                        text: row.name
                        color: row.ink
                    }
                    Loader {
                        anchors.left: icon.right
                        anchors.leftMargin: Kirigami.Units.largeSpacing
                        anchors.right: parent.right
                        anchors.rightMargin: Kirigami.Units.largeSpacing
                        anchors.verticalCenter: parent.verticalCenter
                        active: row.editing
                        sourceComponent: InlineRename {
                            fv: root.fv
                            itemUrl: row.url
                            isDir: row.isDir
                        }
                    }
                }
                Text {
                    width: root.pathWidth
                    height: root.rowHeight
                    visible: root.withPath || root.withTrash
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    horizontalAlignment: Text.AlignLeft
                    text: root.withTrash ? row.originText : row.pathText
                    color: row.inkMuted
                }
                // The matching line of a search inside files ("12: the line").
                Text {
                    width: root.matchWidth
                    height: root.rowHeight
                    visible: root.withMatch
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    text: row.snippetText
                    color: row.inkMuted
                }
                Text {
                    width: root.widths.size
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    text: row.sizeText
                    color: row.inkMuted
                }
                Text {
                    width: root.widths.type
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    text: row.typeText
                    color: row.inkMuted
                }
                Text {
                    width: root.widths.modified
                    height: root.rowHeight
                    leftPadding: Kirigami.Units.largeSpacing
                    verticalAlignment: Text.AlignVCenter
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    text: root.withTrash ? row.deletedText : row.modifiedText
                    color: row.inkMuted
                }
                Repeater {
                    model: [
                        { shown: root.withTags, width: root.widths.tags, text: row.tagsText },
                        { shown: root.withDimensions, width: root.widths.dimensions, text: row.dimensionsText },
                        { shown: root.withDuration, width: root.widths.duration, text: row.durationText },
                        { shown: root.withTaken, width: root.widths.taken, text: row.takenText }
                    ]
                    Text {
                        id: extra
                        required property var modelData
                        width: extra.modelData.shown ? extra.modelData.width : 0
                        height: root.rowHeight
                        visible: extra.modelData.shown
                        leftPadding: Kirigami.Units.largeSpacing
                        rightPadding: Kirigami.Units.largeSpacing
                        verticalAlignment: Text.AlignVCenter
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        horizontalAlignment: Text.AlignLeft
                        text: extra.modelData.text
                        color: row.inkMuted
                    }
                }
            }
            MouseArea {
                id: mouse
                anchors.fill: parent
                // The name being edited takes the mouse; a click elsewhere ends the edit (FolderView).
                enabled: !row.editing
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
                property point start
                property bool narrow: false
                property bool dragged: false
                onPressed: mouseEvent => {
                    if (mouseEvent.button === Qt.RightButton) {
                        const at = mouse.mapToItem(root.fv, mouseEvent.x, mouseEvent.y);
                        root.fv.rowMenu(row.index, at.x, at.y);
                        return;
                    }
                    if (mouseEvent.button === Qt.MiddleButton) {
                        root.fv.middleRow(row.index);
                        return;
                    }
                    start = Qt.point(mouseEvent.x, mouseEvent.y);
                    dragged = false;
                    narrow = !root.fv.pressRow(row.index, mouseEvent.modifiers);
                }
                onPositionChanged: mouseEvent => {
                    if (pressed && !dragged && (pressedButtons & Qt.LeftButton) && Math.hypot(mouseEvent.x - start.x, mouseEvent.y - start.y) > Application.styleHints.startDragDistance) {
                        dragged = true;
                        root.fv.beginDrag();
                    }
                }
                onReleased: mouseEvent => {
                    if (narrow && !dragged) {
                        root.fv.chooseRow(row.index, 0, false);
                    }
                    narrow = false;
                }
                onDoubleClicked: root.fv.activateRow(row.index)
            }
        }
    }
}
