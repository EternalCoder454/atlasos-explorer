import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Atlas.Ui

// Explorer's window. The skeleton: the sidebar's places and a page for the
// place shown. Tabs, the address bar, the command bar and the views come
// with M2 (docs/DESIGN.md, "Window").
AtlasWindow {
    id: root

    // The Rust backend (src/backend.rs); main.cpp sets it.
    required property var backend

    // The folder shown (FolderView.url follows redirects and removed folders).
    readonly property url currentUrl: view.url
    property var backStack: []
    property var forwardStack: []
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
    // Items to select once the folder shown has loaded (FileManager1.ShowItems).
    property var pendingSelect: []
    property bool editingAddress: false
    readonly property var selected: view.selectedUrls
    readonly property bool hasSelection: selected.length > 0
    readonly property bool canWrite: view.folder.canWrite

    FileActions {
        id: actions
        folder: view.folder
        window: root
        onFailed: text => root.launchText = text
        onNavigateRequested: target => root.navigate(target)
    }

    function startAddressEdit() {
        const u = currentUrl;
        address.text = u.toString().startsWith("file:") ? decodeURIComponent(u.toString().substring(7)) : u.toString();
        editingAddress = true;
        address.forceActiveFocus();
        address.selectAll();
    }
    function endAddressEdit() {
        editingAddress = false;
        view.forceActiveFocus();
    }
    function goToAddress() {
        const r = actions.parseAddress(address.text);
        if (r.ok) {
            launchText = "";
            navigate(Qt.url(r.text));
            endAddressEdit();
        } else {
            launchText = r.text;
        }
    }
    function toggleHidden() {
        view.folder.showHidden = !view.folder.showHidden;
        actions.saveShowHidden(view.folder.showHidden);
    }
    // The sort lands after the listing: the selection is made again when the
    // rows move, until the timer ends it.
    function showItems(urls) {
        pendingSelect = urls;
        if (!view.folder.loading) {
            view.selectUrls(urls);
        }
        pendingTimer.restart();
    }
    Timer {
        id: pendingTimer
        interval: 1500
        onTriggered: root.pendingSelect = []
    }

    // Goes to `target`, remembering where it was.
    function navigate(target) {
        if (!target || target.toString() === currentUrl.toString()) {
            return;
        }
        backStack = backStack.concat([currentUrl]);
        forwardStack = [];
        view.url = target;
    }
    function goBack() {
        if (backStack.length === 0) {
            return;
        }
        forwardStack = forwardStack.concat([currentUrl]);
        view.url = backStack[backStack.length - 1];
        backStack = backStack.slice(0, -1);
    }
    function goForward() {
        if (forwardStack.length === 0) {
            return;
        }
        backStack = backStack.concat([currentUrl]);
        view.url = forwardStack[forwardStack.length - 1];
        forwardStack = forwardStack.slice(0, -1);
    }

    title: AtlasApp.name
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
            if (locations.length > 0) {
                root.launchText = "";
                // Tabs and split view come later: the first location is shown.
                if (select || StandardPlaces.isLocalFile(Qt.url(locations[0]))) {
                    // Its folder, with it (and the others in that folder) selected.
                    const folder = StandardPlaces.parentUrl(Qt.url(locations[0]));
                    const same = locations.filter(l => StandardPlaces.parentUrl(Qt.url(l)).toString() === folder.toString());
                    root.pendingSelect = same.map(l => Qt.url(l));
                    root.navigate(folder);
                    root.showItems(root.pendingSelect);
                } else {
                    root.navigate(locations[0]);
                }
            }
        }
        function onInspected(locations) {
            actions.showProperties(locations.map(l => Qt.url(l)));
        }
        function onRefused(text) {
            root.launchText = qsTr("Could not open:") + "\n" + text;
        }
    }

    Connections {
        target: view.folder
        function onLoadingChanged() {
            if (!view.folder.loading && root.pendingSelect.length > 0) {
                view.selectUrls(root.pendingSelect);
            }
        }
        function onLayoutChanged() {
            if (root.pendingSelect.length > 0) {
                view.selectUrls(root.pendingSelect);
            }
        }
    }

    Component.onCompleted: view.folder.showHidden = actions.savedShowHidden()

    // The keys. Those that mean something to a text field are off while the
    // address is being typed.
    Shortcut { sequence: "Ctrl+C"; enabled: !root.editingAddress && root.hasSelection; onActivated: actions.copy(root.selected, false) }
    Shortcut { sequence: "Ctrl+X"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: actions.copy(root.selected, true) }
    Shortcut { sequence: "Ctrl+V"; enabled: !root.editingAddress && root.canWrite; onActivated: actions.paste() }
    Shortcut { sequence: "Ctrl+Z"; enabled: !root.editingAddress; onActivated: actions.undo() }
    Shortcut { sequence: "Delete"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: actions.trash(root.selected) }
    Shortcut { sequence: "Shift+Delete"; enabled: !root.editingAddress && root.hasSelection && root.canWrite; onActivated: actions.deleteForGood(root.selected) }
    Shortcut { sequence: "Ctrl+Shift+N"; enabled: root.canWrite; onActivated: actions.newFolder() }
    Shortcut { sequence: "Ctrl+H"; enabled: !root.editingAddress; onActivated: root.toggleHidden() }
    Shortcut { sequence: "Shift+F4"; onActivated: actions.openTerminal() }
    Shortcut { sequence: "Ctrl+L"; onActivated: root.startAddressEdit() }
    Shortcut { sequence: "F6"; onActivated: root.startAddressEdit() }
    Shortcut { sequence: "Alt+D"; onActivated: root.startAddressEdit() }

    ContextMenu {
        id: viewMenu
        ContextMenuItem { text: qsTr("Details"); radio: true; checkable: true; checked: view.viewMode === "details"; onTriggered: view.viewMode = "details" }
        ContextMenuItem { text: qsTr("Icons"); radio: true; checkable: true; checked: view.viewMode === "icons"; onTriggered: view.viewMode = "icons" }
        ContextMenuItem { text: qsTr("Compact"); radio: true; checkable: true; checked: view.viewMode === "compact"; onTriggered: view.viewMode = "compact" }
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
                checked: view.folder.sortColumn === modelData.column
                onTriggered: view.folder.sortColumn = modelData.column
            }
        }
        ContextMenuSeparator {}
        ContextMenuItem { text: qsTr("Ascending"); radio: true; checkable: true; checked: !view.folder.sortDescending; onTriggered: view.folder.sortDescending = false }
        ContextMenuItem { text: qsTr("Descending"); radio: true; checkable: true; checked: view.folder.sortDescending; onTriggered: view.folder.sortDescending = true }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        AtlasSidebar {
            id: sidebar
            Layout.fillHeight: true
            Layout.preferredWidth: Kirigami.Units.gridUnit * 12.5
            padding: Kirigami.Units.largeSpacing
            spacing: 2

            Repeater {
                model: root.places
                SidebarItem {
                    required property var modelData
                    Layout.fillWidth: true
                    text: modelData.text
                    symbol: modelData.symbol
                    selected: root.place === modelData.key
                    onClicked: root.navigate(StandardPlaces.place(modelData.key))
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

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: Kirigami.Units.smallSpacing
                spacing: Kirigami.Units.smallSpacing

                ToolbarButton {
                    symbol: Symbols.ArrowBack
                    text: qsTr("Back")
                    enabled: root.backStack.length > 0
                    focusable: true
                    onClicked: root.goBack()
                }
                ToolbarButton {
                    symbol: Symbols.ArrowForward
                    text: qsTr("Forward")
                    enabled: root.forwardStack.length > 0
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
                AtlasTextField {
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
                    onClicked: actions.newFolder()
                }
                ToolbarButton {
                    symbol: Symbols.ContentCut
                    text: qsTr("Cut")
                    shortcutText: "Ctrl+X"
                    enabled: root.hasSelection && root.canWrite
                    focusable: true
                    onClicked: actions.copy(root.selected, true)
                }
                ToolbarButton {
                    symbol: Symbols.ContentCopy
                    text: qsTr("Copy")
                    shortcutText: "Ctrl+C"
                    enabled: root.hasSelection
                    focusable: true
                    onClicked: actions.copy(root.selected, false)
                }
                ToolbarButton {
                    symbol: Symbols.ContentPaste
                    text: qsTr("Paste")
                    shortcutText: "Ctrl+V"
                    enabled: root.canWrite && actions.canPaste
                    focusable: true
                    onClicked: actions.paste()
                }
                ToolbarButton {
                    symbol: Symbols.DriveFileRenameOutline
                    text: qsTr("Rename")
                    shortcutText: "F2"
                    enabled: root.selected.length === 1 && root.canWrite
                    focusable: true
                    onClicked: actions.rename(root.selected[0])
                }
                ToolbarButton {
                    symbol: Symbols.Delete
                    text: qsTr("Move to Trash")
                    shortcutText: "Delete"
                    enabled: root.hasSelection && root.canWrite
                    focusable: true
                    onClicked: actions.trash(root.selected)
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

            FolderView {
                id: view
                Layout.fillWidth: true
                Layout.fillHeight: true
                focus: true
                url: StandardPlaces.place("home")
                onNavigateRequested: target => root.navigate(target)
                actions: actions
                // KIO's own prompts apply (Run or open?, untrusted .desktop files).
                onOpenRequested: urls => actions.openUrls(urls)
                onRenameRequested: {
                    if (root.selected.length === 1 && root.canWrite) {
                        actions.rename(root.selected[0]);
                    }
                }
                onContextMenuRequested: urls => actions.contextMenu(urls)
            }
        }
    }
}
