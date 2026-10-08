pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Properties of one or more items, in a Telamon.Ui dialog (it replaces KDE's
// KPropertiesDialog everywhere Files opens one). Four pages:
//   General      name (renamed through the queue), kind, where, size (a folder's
//                is counted only when asked, and can be stopped), dates, the
//                application that opens this kind of file, tags and star rating.
//   Permissions  who may read, write and run, in plain words; for the owner they
//                can be changed (for a folder, with everything inside it).
//   Details      what the file says about itself: size of the picture, length,
//                camera, pages ... (KFileMetaData).
//   Checksums    SHA-256 and the others, calculated on request, stoppable, and
//                compared with a checksum pasted from a download page.
// Everything it shows comes from PropertiesLogic (disk and KIO on workers);
// every change is queued by FileActions, so Undo takes it back.
TelamonDialog {
    id: dialog

    // The window's FileActions.
    required property var actions

    readonly property alias logic: props
    readonly property var general: props.general
    readonly property var perms: props.perms
    readonly property var urls: props.urls
    // The new URL of the item after a rename that was asked for.
    property url pendingUrl
    // The permission boxes changed by the user: "who:bit" -> true or false.
    property var edits: ({})
    property bool recurse: false

    title: general.title ?? qsTr("Properties")
    preferredWidth: Kirigami.Units.gridUnit * 38
    showClose: true

    PropertiesLogic {
        id: props
    }

    function showFor(list) {
        edits = ({});
        recurse = false;
        pendingUrl = Qt.url("");
        pages.currentIndex = 0;
        tabs.currentIndex = 0;
        nameField.text = "";
        pasted.text = "";
        props.load(list);
        open();
    }
    onClosed: props.stopAll()

    // A change that went through the queue (also an undo or redo): read again.
    Connections {
        target: dialog.actions.operations
        function onAttributesChanged() {
            dialog.edits = ({});
            props.reloadAttributes();
        }
        function onJobFinished() {
            // A rename that was asked for: show the item under its new name.
            if (dialog.pendingUrl.toString().length > 0) {
                const target = dialog.pendingUrl;
                dialog.pendingUrl = Qt.url("");
                props.load([target]);
            }
        }
    }
    Connections {
        target: props
        function onGeneralChanged() {
            if (!nameField.activeFocus) {
                nameField.text = props.editableName();
            }
        }
    }

    footerContent: [
        SecondaryButton {
            text: qsTr("Close")
            onClicked: dialog.close()
        }
    ]

    // Header: the icon and what it is.
    RowLayout {
        Layout.fillWidth: true
        spacing: TelamonStyle.spacingLarge
        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.large
            Layout.preferredHeight: Kirigami.Units.iconSizes.large
            source: dialog.general.iconName ?? "document-properties"
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Text {
                Layout.fillWidth: true
                text: dialog.general.title ?? ""
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeTitle
                font.weight: Font.DemiBold
                color: Kirigami.Theme.textColor
                Accessible.role: Accessible.Heading
            }
            Text {
                Layout.fillWidth: true
                text: dialog.general.kind ?? ""
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
            }
        }
        TelamonSpinner {
            visible: props.busy
            running: visible
            Layout.preferredWidth: Kirigami.Units.iconSizes.small
            Layout.preferredHeight: Kirigami.Units.iconSizes.small
        }
    }

    TelamonSegmentedControl {
        id: tabs
        Layout.fillWidth: true
        visible: dialog.general.gone !== true
        model: [qsTr("General"), qsTr("Permissions"), qsTr("Details"), qsTr("Checksums")]
        onActivated: index => pages.currentIndex = index
        Accessible.name: qsTr("Properties page")
    }

    StackLayout {
        id: pages
        Layout.fillWidth: true
        visible: dialog.general.gone !== true

        // ---- General ----
        ColumnLayout {
            id: generalPage
            spacing: TelamonStyle.spacingLarge

            ColumnLayout {
                Layout.fillWidth: true
                visible: dialog.general.single === true && dialog.general.nameEditable === true
                spacing: TelamonStyle.spacing
                Text {
                    text: qsTr("Name")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: TelamonStyle.textMuted
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacingLarge
                    TelamonTextField {
                        id: nameField
                        Layout.fillWidth: true
                        maximumLength: 255
                        property var check: ({
                                "ok": true,
                                "text": ""
                            })
                        errorText: check.ok ? "" : check.text
                        onTextChanged: {
                            if (props.urls.length === 1) {
                                check = dialog.actions.checkName(text, {
                                        "mode": "rename",
                                        "url": props.urls[0]
                                    });
                            }
                        }
                        onAccepted: renameButton.activate()
                        Accessible.name: qsTr("Name")
                    }
                    SecondaryButton {
                        id: renameButton
                        text: qsTr("Rename")
                        enabled: nameField.check.ok && nameField.text.length > 0 && nameField.text !== props.editableName()
                        function activate() {
                            if (!enabled) {
                                return;
                            }
                            dialog.pendingUrl = props.renamedUrl(nameField.text);
                            dialog.actions.renameTo(props.urls[0], nameField.text, false);
                        }
                        onClicked: activate()
                    }
                }
            }

            TelamonDetailGrid {
                Layout.fillWidth: true
                model: {
                    const g = dialog.general;
                    const rows = [];
                    const add = (label, value, mono) => {
                        if (value && value.length > 0) {
                            rows.push({
                                "label": label,
                                "value": value,
                                "mono": mono === true
                            });
                        }
                    };
                    add(qsTr("Kind"), g.kind);
                    add(qsTr("Location"), g.location);
                    add(qsTr("Link to"), g.linkTarget);
                    add(qsTr("Size"), g.sizeText);
                    add(qsTr("Contents"), g.sizeNote);
                    add(qsTr("Created"), g.created);
                    add(qsTr("Modified"), g.modified);
                    add(qsTr("Accessed"), g.accessed);
                    return rows;
                }
            }

            // A folder's size is counted when asked: it can take long on a big
            // folder or a server, and can be stopped.
            RowLayout {
                Layout.fillWidth: true
                visible: props.folderSize.available === true
                spacing: TelamonStyle.spacingLarge
                SecondaryButton {
                    text: props.folderSize.state === "done" ? qsTr("Count Again") : qsTr("Calculate Folder Size")
                    visible: props.folderSize.running !== true
                    onClicked: props.startFolderSize()
                }
                SecondaryButton {
                    text: qsTr("Stop")
                    symbol: Symbols.Stop
                    visible: props.folderSize.running === true
                    onClicked: props.cancelFolderSize()
                }
                TelamonSpinner {
                    visible: props.folderSize.running === true
                    running: visible
                    Layout.preferredWidth: Kirigami.Units.iconSizes.small
                    Layout.preferredHeight: Kirigami.Units.iconSizes.small
                }
                Text {
                    Layout.fillWidth: true
                    text: props.folderSize.text ?? ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                    Accessible.role: Accessible.StaticText
                }
            }

            // The application that opens this kind of file.
            RowLayout {
                Layout.fillWidth: true
                visible: dialog.general.gone !== true
                spacing: TelamonStyle.spacingLarge
                Text {
                    text: qsTr("Opens with")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                }
                TelamonComboBox {
                    id: appBox
                    Layout.fillWidth: true
                    visible: props.openWith.available === true
                    model: props.openWith.apps ?? []
                    textRole: "name"
                    currentIndex: {
                        const apps = props.openWith.apps ?? [];
                        const cur = props.openWith.current;
                        for (let i = 0; i < apps.length; i++) {
                            if (cur && apps[i].id === cur.id) {
                                return i;
                            }
                        }
                        return -1;
                    }
                    placeholderText: qsTr("Choose an application")
                    onActivated: index => props.setDefaultApp(props.openWith.apps[index].id)
                    Accessible.name: qsTr("Application that opens this kind of file")
                }
                Text {
                    Layout.fillWidth: true
                    visible: props.openWith.available !== true
                    text: props.openWith.why ?? ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                }
            }

            // Tags: the colours to put on or take off, the names on the items.
            ColumnLayout {
                Layout.fillWidth: true
                spacing: TelamonStyle.spacing
                Text {
                    text: qsTr("Tags")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: TelamonStyle.textMuted
                }
                Text {
                    Layout.fillWidth: true
                    visible: props.tagsInfo.available === false
                    text: props.tagsInfo.why ?? ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                }
                RowLayout {
                    Layout.fillWidth: true
                    visible: props.tagsInfo.available === true
                    spacing: TelamonStyle.spacingLarge
                    Repeater {
                        model: TagLogic.colours
                        delegate: Item {
                            id: dot
                            required property var modelData
                            readonly property int tagState: {
                                const list = props.tagsInfo.tags ?? [];
                                for (const t of list) {
                                    if (t.name.toLowerCase() === modelData.name.toLowerCase()) {
                                        return t.state;
                                    }
                                }
                                return 0;
                            }
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 1.6
                            Layout.preferredHeight: Kirigami.Units.gridUnit * 1.6
                            Rectangle {
                                anchors.centerIn: parent
                                width: Kirigami.Units.gridUnit * 1.3
                                height: width
                                radius: width / 2
                                color: dot.modelData.colour
                                border.width: dotArea.containsMouse || dot.activeFocus ? 2 : 0
                                border.color: TelamonStyle.text
                            }
                            Symbol {
                                anchors.centerIn: parent
                                visible: dot.tagState !== 0
                                icon: dot.tagState === 2 ? Symbols.Check : Symbols.Remove
                                size: Kirigami.Units.iconSizes.small
                                color: "white" // telamon-lint: allow-raw (a mark on a colour tag's dot)
                            }
                            MouseArea {
                                id: dotArea
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: dot.toggle()
                            }
                            function toggle() {
                                dialog.actions.toggleTag(props.urls, modelData.name, tagState !== 2);
                            }
                            activeFocusOnTab: true
                            Keys.onSpacePressed: toggle()
                            Keys.onReturnPressed: toggle()
                            TelamonToolTip {
                                text: dot.modelData.name
                                shown: dotArea.containsMouse
                            }
                            Accessible.role: Accessible.CheckBox
                            Accessible.name: modelData.name
                            Accessible.checkable: true
                            Accessible.checked: tagState === 2
                            Accessible.onPressAction: toggle()
                        }
                    }
                    SecondaryButton {
                        text: qsTr("New Tag…")
                        onClicked: dialog.actions.newTag(props.urls)
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                }
                Flow {
                    Layout.fillWidth: true
                    visible: props.tagsInfo.available === true && (props.tagsInfo.hasTags === true)
                    spacing: TelamonStyle.spacing
                    Repeater {
                        model: props.tagsInfo.tags ?? []
                        delegate: TelamonChip {
                            required property var modelData
                            text: modelData.state === 1 ? qsTr("%1 (some items)").arg(modelData.text) : modelData.text
                            closable: true
                            maximumWidth: Kirigami.Units.gridUnit * 18
                            onCloseRequested: dialog.actions.toggleTag(props.urls, modelData.name, false)
                        }
                    }
                }
            }

            // Star rating (Baloo's rating, so KDE's apps show the same stars).
            RowLayout {
                Layout.fillWidth: true
                visible: props.ratingAvailable
                spacing: TelamonStyle.spacingLarge
                Text {
                    text: qsTr("Rating")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                }
                TelamonRating {
                    id: stars
                    readOnly: false
                    value: Math.max(0, props.rating) / 2
                    onEdited: dialog.actions.setRating(props.urls, Math.round(value * 2))
                    Accessible.name: qsTr("Rating")
                }
                TextButton {
                    text: qsTr("Clear")
                    visible: props.rating !== 0
                    onClicked: dialog.actions.setRating(props.urls, 0)
                }
                Text {
                    visible: props.rating < 0
                    text: qsTr("The items have different ratings.")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: TelamonStyle.textMuted
                }
                Item {
                    Layout.fillWidth: true
                }
            }
        }

        // ---- Permissions ----
        ColumnLayout {
            id: permsPage
            spacing: TelamonStyle.spacingLarge

            Text {
                Layout.fillWidth: true
                visible: dialog.perms.available !== true
                text: dialog.perms.why ?? ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
            }
            Text {
                Layout.fillWidth: true
                visible: dialog.perms.available === true && (dialog.perms.sentence ?? "").length > 0
                text: dialog.perms.sentence ?? ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
            }

            GridLayout {
                Layout.fillWidth: true
                visible: dialog.perms.available === true
                columns: 4
                columnSpacing: TelamonStyle.spacingLarge * 2
                rowSpacing: TelamonStyle.spacing

                Item {}
                Repeater {
                    model: [qsTr("Read"), qsTr("Write"), dialog.perms.runLabel ?? qsTr("Run")]
                    delegate: Text {
                        required property string modelData
                        text: modelData
                        textFormat: Text.PlainText
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: TelamonStyle.textMuted
                    }
                }
                Repeater {
                    model: (dialog.perms.rows ?? []).length * 4
                    delegate: Loader {
                        id: cellLoader
                        required property int index
                        readonly property int rowIndex: Math.floor(index / 4)
                        readonly property int column: index % 4
                        readonly property var row: (dialog.perms.rows ?? [])[rowIndex]
                        Layout.alignment: Qt.AlignVCenter
                        sourceComponent: column === 0 ? labelCell : boxCell
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                visible: dialog.perms.canRecurse === true
                TelamonCheckBox {
                    text: qsTr("Also change everything inside the folders")
                    checked: dialog.recurse
                    enabled: dialog.perms.editable === true
                    onToggled: dialog.recurse = checked
                }
            }
            Text {
                Layout.fillWidth: true
                visible: dialog.perms.canRecurse === true && dialog.recurse
                text: qsTr("Files inside keep their Run setting. Only what you changed here is changed.")
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
            }
            Text {
                Layout.fillWidth: true
                visible: dialog.perms.available === true && dialog.perms.editable !== true
                text: dialog.perms.why ?? ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
            }
            Text {
                Layout.fillWidth: true
                visible: (dialog.perms.octal ?? "").length > 0
                text: qsTr("Mode %1 (%2)").arg(dialog.perms.octal ?? "").arg(dialog.perms.symbolic ?? "")
                textFormat: Text.PlainText
                font.family: TelamonStyle.monoFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
            }
            RowLayout {
                Layout.fillWidth: true
                visible: dialog.perms.editable === true
                spacing: TelamonStyle.spacingLarge
                Item {
                    Layout.fillWidth: true
                }
                SecondaryButton {
                    text: qsTr("Revert")
                    enabled: dialog.changeCount > 0 || dialog.recurse
                    onClicked: {
                        dialog.edits = ({});
                        dialog.recurse = false;
                    }
                }
                PrimaryButton {
                    text: qsTr("Apply")
                    enabled: dialog.changeCount > 0
                    onClicked: dialog.applyPermissions()
                }
            }
        }

        // ---- Details ----
        ColumnLayout {
            id: detailsPage
            spacing: TelamonStyle.spacingLarge
            TelamonDetailGrid {
                Layout.fillWidth: true
                visible: props.details.length > 0
                model: props.details
            }
            Text {
                Layout.fillWidth: true
                visible: props.details.length === 0
                text: props.detailsLoaded ? (dialog.general.single === true && dialog.general.anyDir !== true && dialog.general.local === true ? qsTr("Nothing more is known about this file.") : qsTr("Details are shown for one file on this computer.")) : qsTr("Reading…")
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
            }
        }

        // ---- Checksums ----
        ColumnLayout {
            id: sumsPage
            spacing: TelamonStyle.spacingLarge

            Text {
                Layout.fillWidth: true
                visible: props.checksum.available !== true
                text: props.checksum.why ?? ""
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: TelamonStyle.textMuted
            }
            ColumnLayout {
                Layout.fillWidth: true
                visible: props.checksum.available === true
                spacing: TelamonStyle.spacingLarge

                RowLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacingLarge
                    TelamonComboBox {
                        id: algBox
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 10
                        model: ["SHA-256", "SHA-1", "MD5", "SHA-512"]
                        enabled: props.checksum.running !== true
                        Accessible.name: qsTr("Kind of checksum")
                    }
                    PrimaryButton {
                        text: qsTr("Calculate")
                        visible: props.checksum.running !== true
                        onClicked: props.startChecksum(algBox.currentIndex)
                    }
                    SecondaryButton {
                        text: qsTr("Stop")
                        symbol: Symbols.Stop
                        visible: props.checksum.running === true
                        onClicked: props.cancelChecksum()
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                }
                TelamonProgressBar {
                    Layout.fillWidth: true
                    visible: props.checksum.running === true
                    value: props.checksum.progress ?? 0
                    text: Math.round((props.checksum.progress ?? 0) * 100) + " %"
                    Accessible.name: qsTr("Calculating the checksum")
                }
                Text {
                    Layout.fillWidth: true
                    visible: (props.checksum.error ?? "").length > 0
                    text: props.checksum.error ?? ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: Kirigami.Theme.negativeTextColor
                    Accessible.role: Accessible.AlertMessage
                }
                TelamonDetailGrid {
                    Layout.fillWidth: true
                    visible: (props.checksum.results ?? []).length > 0
                    model: {
                        const rows = [];
                        for (const r of (props.checksum.results ?? [])) {
                            rows.push({
                                "label": r.name,
                                "value": r.hex,
                                "mono": true,
                                "copyable": true
                            });
                        }
                        return rows;
                    }
                }
                Text {
                    text: qsTr("Compare with")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    color: TelamonStyle.textMuted
                }
                TelamonTextField {
                    id: pasted
                    Layout.fillWidth: true
                    placeholderText: qsTr("Paste a checksum here")
                    onTextChanged: props.compareWith(text)
                    Accessible.name: qsTr("Checksum to compare with")
                }
                RowLayout {
                    Layout.fillWidth: true
                    visible: (props.checksum.compare?.state ?? "").length > 0
                    spacing: TelamonStyle.spacing
                    readonly property string verdict: props.checksum.compare?.state ?? ""
                    Symbol {
                        icon: parent.verdict === "match" ? Symbols.CheckCircle : (parent.verdict === "mismatch" || parent.verdict === "invalid" ? Symbols.Error : Symbols.Info)
                        size: Kirigami.Units.iconSizes.smallMedium
                        color: parent.verdict === "match" ? Kirigami.Theme.positiveTextColor : (parent.verdict === "mismatch" || parent.verdict === "invalid" ? Kirigami.Theme.negativeTextColor : TelamonStyle.textMuted)
                    }
                    Text {
                        Layout.fillWidth: true
                        text: props.checksum.compare?.text ?? ""
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeBody
                        color: Kirigami.Theme.textColor
                        Accessible.role: Accessible.AlertMessage
                        Accessible.name: text
                    }
                }
            }
        }
    }

    // A not-found item says so instead of the pages.
    Text {
        Layout.fillWidth: true
        visible: dialog.general.gone === true
        text: dialog.general.kind ?? ""
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: TelamonStyle.textMuted
    }

    // ---- Permissions: the boxes ----

    Component {
        id: labelCell
        Text {
            text: cellLoaderParent.row?.label ?? ""
            textFormat: Text.PlainText
            elide: Text.ElideRight
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: Kirigami.Theme.textColor
            readonly property var cellLoaderParent: parent
        }
    }
    Component {
        id: boxCell
        TelamonCheckBox {
            id: box
            readonly property var holder: parent
            readonly property int who: holder.row?.who ?? 0
            readonly property int bit: holder.column - 1
            readonly property string key: who + ":" + bit
            readonly property string field: ["read", "write", "run"][bit]
            readonly property int original: holder.row ? holder.row[field] : 0
            readonly property bool edited: dialog.edits[key] !== undefined
            enabled: dialog.perms.editable === true
            tristate: false
            checkState: edited ? (dialog.edits[key] ? Qt.Checked : Qt.Unchecked) : (original === 2 ? Qt.Checked : (original === 1 ? Qt.PartiallyChecked : Qt.Unchecked))
            nextCheckState: () => checkState === Qt.Checked ? Qt.Unchecked : Qt.Checked
            onToggled: {
                const e = Object.assign({}, dialog.edits);
                const wanted = checkState === Qt.Checked;
                // Back to what it was: not a change.
                if (original !== 1 && (original === 2) === wanted) {
                    delete e[key];
                } else {
                    e[key] = wanted;
                }
                dialog.edits = e;
            }
            Accessible.name: (holder.row?.label ?? "") + ", " + ["Read", "Write", "Run"][bit]
        }
    }

    // The bits to turn on and off for the boxes the user changed, and how many.
    readonly property var changes: {
        let set = 0, clear = 0, count = 0;
        for (const k of Object.keys(edits)) {
            const [who, bit] = k.split(":").map(Number);
            // Owner 0o400 ... others 0o001; bit 0 read (4), 1 write (2), 2 run (1).
            const mask = (4 >> bit) << ((2 - who) * 3);
            if (edits[k]) {
                set |= mask;
            } else {
                clear |= mask;
            }
            count++;
        }
        return ({
                "set": set,
                "clear": clear,
                "count": count
            });
    }
    readonly property int changeCount: changes.count

    function applyPermissions() {
        if (changes.count === 0) {
            return;
        }
        dialog.actions.setPermissions(props.urls, changes.set, changes.clear, dialog.recurse);
    }
}
