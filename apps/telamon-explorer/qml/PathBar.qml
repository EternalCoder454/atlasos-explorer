pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Templates as T
import QtQml
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The path of the folder shown, as clickable segments with a chevron after
// each: a click on a segment goes there (Ctrl+click or a middle click opens it
// in a new background tab), the chevron lists that folder's subfolders, and
// files dropped on a segment are moved into it (copied with Ctrl held). A
// click on the empty part, or startEdit(), turns the bar into a text field
// with the full path: Enter goes, Escape comes back, and a partial folder name
// offers completions (Up and Down choose, Tab takes one). Listing happens in
// LocationLogic, off this thread; the segments are made by the Rust core.
//
// It is Telamon.Ui's TelamonBreadcrumb with what Files needs added (a menu
// per chevron, drops, middle click, editing); when the framework's gains
// those, this file goes (docs/DESIGN.md, "Framework gaps").
FocusScope {
    id: bar

    // The folder shown.
    property url location
    // Whether the subfolder menus list hidden folders.
    property bool showHidden: false
    // The window's FileActions: drops go through it.
    required property var actions
    readonly property var segments: LocationLogic.segments(location)
    // Whether the text field is shown instead of the segments.
    property bool editing: false
    property alias text: field.text

    // A segment was clicked, or a subfolder chosen.
    signal navigateRequested(url target)
    // Ctrl+click or a middle click.
    signal openInNewTabRequested(url target)
    // Enter in the field, with the text.
    signal addressAccepted(string text)
    // The field was left, by Escape or by moving the focus away.
    signal editEnded
    // Any key that changed the text; the window clears an old refusal.
    signal addressEdited

    function startEdit() {
        field.text = LocationLogic.editText(bar.location);
        bar.editing = true;
        field.forceActiveFocus();
        field.selectAll();
        priv.completions = [];
    }
    function endEdit() {
        if (!bar.editing) {
            return;
        }
        bar.editing = false;
        priv.completions = [];
        priv.chosen = -1;
        completionTimer.stop();
        bar.editEnded();
    }

    implicitWidth: Kirigami.Units.gridUnit * 24
    implicitHeight: TelamonStyle.controlHeight
    Accessible.role: Accessible.ToolBar
    //: Spoken name of the path bar
    Accessible.name: qsTr("Path")

    QtObject {
        id: priv
        readonly property real chevronWidth: Math.round(Kirigami.Units.gridUnit * 1.1)
        readonly property real moreWidth: Math.round(Kirigami.Units.gridUnit * 1.8) + chevronWidth
        // Bumped when a segment's width is known or changes, to lay out again.
        property int revision: 0
        // The first segment shown; those before it are in the "..." menu.
        readonly property int firstShown: {
            priv.revision;
            const n = bar.segments.length;
            const avail = strip.width;
            let total = 0;
            for (let i = 0; i < n; ++i) {
                total += priv.widthOf(i);
            }
            if (total <= avail || n < 2) {
                return 0;
            }
            let used = priv.moreWidth + priv.widthOf(n - 1);
            let from = n - 1;
            while (from > 1 && used + priv.widthOf(from - 1) <= avail) {
                used += priv.widthOf(from - 1);
                --from;
            }
            return from;
        }
        // The subfolder menu: rows of {label, url} or an {info} line.
        property var menuRows: []
        property int menuFor: -1
        property int menuSerial: -1
        // What the drop is going to do, shown under the segment.
        property string hint
        property real hintX: 0
        // Completions of the typed text, the one chosen with the arrow keys
        // (-1 for none), and the serial of the request they answer.
        property var completions: []
        property int chosen: -1
        property int completionSerial: -1
        property string completionFor
        onCompletionsChanged: {
            if (bar.editing && priv.completions.length > 0) {
                popup.open();
            } else {
                popup.close();
            }
        }

        function widthOf(i) {
            const item = items.itemAt(i);
            return item ? item.implicitWidth : 0;
        }
        function open(target) {
            if (TabLogic.controlHeld()) {
                bar.openInNewTabRequested(target);
            } else {
                bar.navigateRequested(target);
            }
        }
        function takeCompletion(i) {
            const t = priv.completions[i];
            // A list made for text that has since changed is not taken.
            if (t === undefined || priv.completionFor !== field.text) {
                return false;
            }
            field.text = t;
            field.cursorPosition = t.length;
            priv.completions = [];
            priv.chosen = -1;
            bar.addressEdited();
            completionTimer.restart();
            return true;
        }
    }

    // The empty part: a click edits the path.
    Rectangle {
        anchors.fill: parent
        visible: !bar.editing
        radius: TelamonStyle.radiusSmall
        color: TelamonStyle.control
        border.width: 1
        border.color: TelamonStyle.controlBorder
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.IBeamCursor
            onClicked: bar.startEdit()
        }
    }

    Item {
        id: strip
        anchors.fill: parent
        anchors.margins: 2
        visible: !bar.editing
        clip: true

        Row {
            id: row
            height: parent.height

            // The "..." button: the folders that don't fit.
            Item {
                id: more
                visible: priv.firstShown > 0
                width: visible ? priv.moreWidth : 0
                height: row.height
                T.AbstractButton {
                    id: moreButton
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    width: priv.moreWidth - priv.chevronWidth
                    height: Math.round(Kirigami.Units.gridUnit * 1.6)
                    hoverEnabled: true
                    focusPolicy: Qt.TabFocus
                    Accessible.name: qsTr("Hidden folders")
                    onClicked: hiddenMenu.popup(moreButton, 0, moreButton.height + TelamonStyle.spacingSmall)
                    background: Rectangle {
                        radius: TelamonStyle.radiusSmall
                        color: moreButton.down || hiddenMenu.visible ? TelamonStyle.pressed : moreButton.hovered ? TelamonStyle.hover : "transparent"
                    }
                    contentItem: Item {
                        Symbol {
                            anchors.centerIn: parent
                            icon: Symbols.MoreHoriz
                            size: Kirigami.Units.iconSizes.smallMedium
                            color: TelamonStyle.textMuted
                        }
                    }
                }
                Symbol {
                    anchors.left: moreButton.right
                    anchors.leftMargin: Math.round((priv.chevronWidth - width) / 2)
                    anchors.verticalCenter: parent.verticalCenter
                    icon: LayoutMirroring.enabled ? Symbols.ChevronLeft : Symbols.ChevronRight
                    size: Kirigami.Units.iconSizes.smallMedium
                    color: TelamonStyle.textMuted
                }
            }

            Repeater {
                id: items
                model: bar.segments
                onItemAdded: priv.revision++
                onItemRemoved: priv.revision++

                delegate: Item {
                    id: seg
                    required property int index
                    required property var modelData
                    readonly property bool last: seg.index === bar.segments.length - 1
                    readonly property bool menuOpen: subMenu.visible && priv.menuFor === seg.index

                    visible: seg.index >= priv.firstShown
                    width: visible ? seg.implicitWidth : 0
                    height: row.height
                    implicitWidth: button.implicitWidth + priv.chevronWidth
                    onImplicitWidthChanged: priv.revision++

                    T.AbstractButton {
                        id: button
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.min(implicitWidth, seg.implicitWidth - priv.chevronWidth)
                        height: Math.round(Kirigami.Units.gridUnit * 1.6)
                        leftPadding: TelamonStyle.spacingLarge
                        rightPadding: TelamonStyle.spacingLarge
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        text: seg.modelData.label
                        implicitWidth: Math.min(Math.round(Kirigami.Units.gridUnit * 16), label.implicitWidth + leftPadding + rightPadding)
                        Accessible.role: Accessible.Button
                        Accessible.name: button.text
                        //: Spoken hint on the last segment of the path bar: it is where you are now
                        Accessible.description: seg.last ? qsTr("Current location") : ""
                        onClicked: priv.open(seg.modelData.url)

                        background: Item {
                            Rectangle {
                                id: pill
                                anchors.fill: parent
                                radius: TelamonStyle.radiusSmall
                                color: dz.containsDrag ? TelamonStyle.selection : button.down ? TelamonStyle.pressed : button.hovered ? TelamonStyle.hover : "transparent"
                                border.width: dz.containsDrag ? 1 : 0
                                border.color: TelamonStyle.accent
                            }
                            TelamonFocusRing {
                                radius: pill.radius + gap
                                shown: button.visualFocus
                            }
                        }
                        contentItem: Text {
                            id: label
                            text: button.text
                            font.family: TelamonStyle.fontFamily
                            font.pointSize: TelamonStyle.fontSizeBody
                            font.bold: seg.last
                            color: seg.last ? TelamonStyle.text : TelamonStyle.textMuted
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            verticalAlignment: Text.AlignVCenter
                        }

                        // A middle click opens the folder in a new background tab.
                        MouseArea {
                            anchors.fill: parent
                            acceptedButtons: Qt.MiddleButton
                            onClicked: bar.openInNewTabRequested(seg.modelData.url)
                        }

                        // Files dropped here go into the folder.
                        DropArea {
                            id: dz
                            anchors.fill: parent
                            property bool copy: false
                            // Moves unless Ctrl is held; copies when the
                            // source offers nothing else. Setting the action
                            // is what the drag cursor shows.
                            function decide(drag) {
                                const canMove = (drag.supportedActions & Qt.MoveAction) !== 0;
                                const canCopy = (drag.supportedActions & Qt.CopyAction) !== 0;
                                copy = canCopy && (!canMove || bar.actions.copyKeyHeld());
                                drag.accepted = drag.hasUrls && (canMove || canCopy);
                                drag.action = copy ? Qt.CopyAction : Qt.MoveAction;
                                if (drag.accepted) {
                                    priv.hint = (copy ? qsTr("Copy to %1") : qsTr("Move to %1")).arg(seg.modelData.label);
                                    priv.hintX = seg.mapToItem(bar, 0, 0).x;
                                }
                            }
                            onEntered: drag => decide(drag)
                            onPositionChanged: drag => decide(drag)
                            onExited: priv.hint = ""
                            onDropped: drop => {
                                priv.hint = "";
                                if (!drop.hasUrls) {
                                    return;
                                }
                                decide(drop);
                                // The move or copy is done here, so the source is
                                // told it was a copy and leaves the files alone.
                                drop.accept(Qt.CopyAction);
                                bar.actions.dropTo(drop.urls, seg.modelData.url, copy);
                            }
                        }
                    }

                    // The chevron: this folder's subfolders.
                    T.AbstractButton {
                        id: chevron
                        anchors.left: button.right
                        anchors.verticalCenter: parent.verticalCenter
                        width: priv.chevronWidth
                        height: Math.round(Kirigami.Units.gridUnit * 1.6)
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        Accessible.role: Accessible.ButtonMenu
                        //: Spoken name of a chevron in the path bar: %1 is a folder's name
                        Accessible.name: qsTr("Folders in %1").arg(seg.modelData.label)
                        onClicked: bar.openSubfolders(seg.index, chevron)
                        background: Rectangle {
                            radius: TelamonStyle.radiusSmall
                            color: seg.menuOpen || chevron.down ? TelamonStyle.pressed : chevron.hovered ? TelamonStyle.hover : "transparent"
                        }
                        contentItem: Item {
                            Symbol {
                                anchors.centerIn: parent
                                icon: LayoutMirroring.enabled ? Symbols.ChevronLeft : Symbols.ChevronRight
                                size: Kirigami.Units.iconSizes.smallMedium
                                color: TelamonStyle.textMuted
                                rotation: seg.menuOpen ? 90 : 0
                                Behavior on rotation {
                                    NumberAnimation {
                                        duration: TelamonStyle.durationShort
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // What a drop would do, under the segment.
    Rectangle {
        visible: priv.hint.length > 0
        z: 10
        x: Math.max(0, Math.min(priv.hintX, bar.width - width))
        y: bar.height + TelamonStyle.spacingSmall
        radius: TelamonStyle.radiusSmall
        color: TelamonStyle.floatingBackground
        border.width: 1
        border.color: TelamonStyle.separator
        implicitWidth: hintText.implicitWidth + TelamonStyle.spacingLarge * 2
        implicitHeight: hintText.implicitHeight + TelamonStyle.spacing
        width: implicitWidth
        height: implicitHeight
        Text {
            id: hintText
            anchors.centerIn: parent
            text: priv.hint
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            color: TelamonStyle.text
            textFormat: Text.PlainText
        }
    }

    // ---- The subfolder menu ----
    function openSubfolders(i, anchor) {
        const target = bar.segments[i].url;
        priv.menuFor = i;
        priv.menuRows = [
            {
                "info": qsTr("Loading…")
            }
        ];
        priv.menuSerial = LocationLogic.listSubfolders(target, bar.showHidden);
        subMenu.popup(anchor, 0, anchor.height + TelamonStyle.spacingSmall);
    }

    Connections {
        target: LocationLogic
        function onSubfoldersListed(serial, rows, more, error) {
            if (serial !== priv.menuSerial) {
                return;
            }
            if (error.length > 0) {
                priv.menuRows = [
                    {
                        "info": error
                    }
                ];
                return;
            }
            const out = rows.slice();
            if (out.length === 0) {
                out.push({
                    "info": qsTr("No folders")
                });
            } else if (more > 0) {
                out.push({
                    "info": qsTr("%1 more not shown").arg(more)
                });
            }
            priv.menuRows = out;
        }
        function onCompletionsReady(serial, texts, error) {
            if (serial !== priv.completionSerial || !bar.editing) {
                return;
            }
            // Nothing to offer when the one answer is what is already typed.
            priv.completions = texts.length === 1 && texts[0] === priv.completionFor ? [] : texts;
            priv.chosen = -1;
        }
    }

    ContextMenu {
        id: subMenu
        Instantiator {
            model: priv.menuRows
            delegate: ContextMenuItem {
                required property var modelData
                readonly property bool info: modelData.info !== undefined
                // A menu row reads "&" as a mnemonic marker: names say "&&".
                text: (info ? modelData.info : modelData.label).replace(/&/g, "&&")
                enabled: !info
                onTriggered: if (!info) priv.open(modelData.url)
            }
            onObjectAdded: (index, object) => subMenu.insertItem(index, object)
            onObjectRemoved: (index, object) => subMenu.removeItem(object)
        }
    }

    // The segments that don't fit, nearest to the folder shown last.
    ContextMenu {
        id: hiddenMenu
        Instantiator {
            model: priv.firstShown
            delegate: ContextMenuItem {
                required property int index
                text: (bar.segments[priv.firstShown - 1 - index]?.label ?? "").replace(/&/g, "&&")
                onTriggered: priv.open(bar.segments[priv.firstShown - 1 - index].url)
            }
            onObjectAdded: (index, object) => hiddenMenu.insertItem(index, object)
            onObjectRemoved: (index, object) => hiddenMenu.removeItem(object)
        }
    }

    // ---- The text field ----
    TelamonTextField {
        id: field
        anchors.fill: parent
        visible: bar.editing
        Accessible.name: qsTr("Path")

        Keys.onEscapePressed: bar.endEdit()
        onAccepted: {
            // An arrow-chosen completion is what Enter goes to.
            if (priv.chosen >= 0 && priv.completions[priv.chosen] !== undefined && priv.completionFor === field.text) {
                field.text = priv.completions[priv.chosen];
            }
            priv.completions = [];
            priv.chosen = -1;
            bar.addressAccepted(field.text);
        }
        onTextEdited: {
            priv.chosen = -1;
            bar.addressEdited();
            completionTimer.restart();
        }
        onActiveFocusChanged: if (!activeFocus) bar.endEdit()
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Tab && !(event.modifiers & Qt.ShiftModifier)) {
                // Tab takes a completion; it never leaves the field.
                if (priv.completions.length > 0) {
                    priv.takeCompletion(priv.chosen >= 0 ? priv.chosen : 0);
                }
                event.accepted = true;
            } else if (event.key === Qt.Key_Down && priv.completions.length > 0) {
                priv.chosen = Math.min(priv.chosen + 1, priv.completions.length - 1);
                event.accepted = true;
            } else if (event.key === Qt.Key_Up && priv.completions.length > 0) {
                priv.chosen = Math.max(priv.chosen - 1, 0);
                event.accepted = true;
            }
        }
    }

    // Asks for completions a moment after the last key; a server gets longer.
    Timer {
        id: completionTimer
        interval: bar.location.toString().startsWith("file:") && !field.text.includes("://") ? 40 : 300
        onTriggered: {
            priv.completionFor = field.text;
            priv.completionSerial = LocationLogic.complete(field.text, bar.location);
        }
    }

    T.Popup {
        id: popup
        parent: field
        y: field.height + TelamonStyle.spacingSmall
        x: 0
        width: field.width
        implicitHeight: contentItem.implicitHeight + topPadding + bottomPadding
        padding: TelamonStyle.spacingSmall
        margins: TelamonStyle.spacingSmall
        // The list is only a view of the field: it never takes the focus.
        focus: false
        modal: false
        closePolicy: T.Popup.NoAutoClose

        contentItem: ListView {
            implicitHeight: Math.min(contentHeight, Math.round(Kirigami.Units.gridUnit * 1.8) * 8)
            model: popup.visible ? priv.completions : []
            currentIndex: priv.chosen
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
            delegate: T.ItemDelegate {
                id: item
                required property string modelData
                required property int index
                width: ListView.view ? ListView.view.width : implicitWidth
                implicitHeight: Math.round(Kirigami.Units.gridUnit * 1.8)
                leftPadding: Kirigami.Units.largeSpacing
                rightPadding: Kirigami.Units.largeSpacing
                hoverEnabled: true
                focusPolicy: Qt.NoFocus
                highlighted: priv.chosen === item.index
                onClicked: {
                    priv.takeCompletion(item.index);
                    field.forceActiveFocus();
                }
                Accessible.role: Accessible.ListItem
                Accessible.name: modelData
                background: Rectangle {
                    radius: TelamonStyle.radiusSmall
                    // The pointer only lights a row; Enter goes to the one chosen with the arrow keys.
                    color: item.highlighted ? TelamonStyle.alpha(TelamonStyle.accent, item.down ? 0.28 : 0.18) : item.hovered ? TelamonStyle.hover : "transparent"
                }
                contentItem: Text {
                    text: item.modelData
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: Kirigami.Theme.textColor
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideMiddle
                }
            }
        }

        background: Item {
            Rectangle {
                anchors.fill: parent
                radius: TelamonStyle.radius
                color: TelamonStyle.floatingBackground
                border.width: 1
                border.color: TelamonStyle.separator
            }
        }
    }
}
