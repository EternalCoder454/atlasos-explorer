pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One pane of a tab: a folder with its own back and forward history, view
// mode, selection, search and scroll position (they live in the FolderView,
// which stays alive while the tab is hidden). A tab has one pane, or two side
// by side (split view, F3: FilesTabPage); with two, each pane shows its own
// path bar above the folder, and the one that has the keyboard has an accent
// line. The window owns the strip and the toolbar; the tab answers navigate,
// goBack and goForward for the pane that is active.
FocusScope {
    id: page

    // The window's FileActions (jobs, dialogs, context menus).
    required property var actions
    // The tab shows two panes; this one is the active one (it has the keyboard).
    property bool split: false
    property bool paneActive: false
    // This pane is not the first: a line separates it from the one before.
    property bool leadingLine: false
    // Where the tab starts, how it is shown and what its history was.
    property url startUrl
    // A view to show the folder in instead of the one it remembers ("": its own).
    property string startViewMode: ""
    property var startBack: []
    property var startForward: []
    // A restored background tab loads its folder when it is first shown, so a
    // session of many tabs (or of slow servers) doesn't list them all at start.
    property bool lazy: false
    // Items to select once the folder has loaded (ShowItems, --select).
    property var pendingSelect: []

    // In a row of panes, each takes the same share.
    Layout.fillWidth: true
    Layout.fillHeight: true
    Layout.preferredWidth: 1
    Layout.minimumWidth: 0

    property alias view: view
    // The path bar of the pane's own header (shown while the tab is split).
    property alias pathBar: headerBar
    // The tab's search: words, scope and chips live here, so each tab has its own.
    property alias search: tabSearch
    property bool loaded: false
    property var backStack: []
    property var forwardStack: []
    // The folder shown, or where the tab will start while it is not loaded yet.
    readonly property url location: loaded ? view.url : startUrl
    readonly property string title: StandardPlaces.tabTitle(location)
    readonly property string toolTip: StandardPlaces.displayLocation(location)
    readonly property bool canGoBack: backStack.length > 0
    readonly property bool canGoForward: forwardStack.length > 0

    // A folder to open in a new background tab.
    signal openInNewTab(url target)
    // The user moved this tab to another folder (not a restore or a launch).
    signal navigated
    // Open File Location on search results: their URLs.
    signal openLocation(var urls)
    // Space in the view: Quick Look for the selected file.
    signal quickLookRequested
    // A right click or the Menu key: the items (none: the folder's background)
    // and where, in the view's coordinates.
    signal contextMenuRequested(var urls, real x, real y)
    // "Connect to Server…" was chosen on the Network page.
    signal connectRequested
    // The pane took the keyboard or a click: it is the active one.
    signal activated
    // The pane's × was clicked.
    signal closeRequested
    // Enter in the pane's own path bar, with the text; and a key that changed it.
    signal addressAccepted(string text)
    signal addressEdited

    // Home and Network are pages Files draws itself: this one shows while the
    // tab is there, and the folder view stays out of sight.
    readonly property string pageKind: view.folder.pageKind
    // The keyboard goes to what the tab shows: its folder, or its page.
    function focusContent() {
        if (pageKind.length > 0) {
            if (pageLoader.item) {
                pageLoader.item.forceActiveFocus();
            }
        } else {
            view.forceActiveFocus();
        }
    }

    function load() {
        if (loaded) {
            return;
        }
        loaded = true;
        view.folder.showHidden = actions.savedShowHidden();
        backStack = startBack;
        forwardStack = startForward;
        view.url = startUrl;
        if (startViewMode.length > 0) {
            // A duplicated or reopened tab looks as it did; nothing is remembered for that.
            view.restoring = true;
            view.viewMode = startViewMode;
            view.restoring = false;
        }
    }

    // Goes to `target`, remembering where it was.
    function navigate(target) {
        load();
        // Going to a folder, even the one shown, ends a search.
        tabSearch.clear();
        if (!target || target.toString().replace(/\/+$/, "") === view.url.toString().replace(/\/+$/, "")) {
            return;
        }
        backStack = backStack.concat([view.url]);
        forwardStack = [];
        view.url = target;
        navigated();
    }
    function goBack() {
        goBackBy(1);
    }
    function goForward() {
        goForwardBy(1);
    }
    // Goes `n` places back (the places passed stay in the forward list, so
    // Forward walks them again).
    function goBackBy(n) {
        const len = backStack.length;
        n = Math.min(n, len);
        if (n < 1) {
            return;
        }
        const passed = backStack.slice(len - n + 1).reverse();
        forwardStack = forwardStack.concat([view.url], passed);
        view.url = backStack[len - n];
        backStack = backStack.slice(0, len - n);
        navigated();
    }
    function goForwardBy(n) {
        const len = forwardStack.length;
        n = Math.min(n, len);
        if (n < 1) {
            return;
        }
        const passed = forwardStack.slice(len - n + 1).reverse();
        backStack = backStack.concat([view.url], passed);
        view.url = forwardStack[len - n];
        forwardStack = forwardStack.slice(0, len - n);
        navigated();
    }
    // The last places of the lists, nearest first, as {url, steps} for the
    // menus of Back and Forward.
    function backPlaces() {
        return recent(backStack);
    }
    function forwardPlaces() {
        return recent(forwardStack);
    }
    function recent(stack) {
        const out = [];
        for (let i = stack.length - 1; i >= 0 && out.length < LocationLogic.historyRows; --i) {
            out.push({
                "url": stack[i],
                "steps": stack.length - i
            });
        }
        return out;
    }
    // Shows `target` without a history entry: the tab was never used (a first
    // launch with a folder to show).
    function showHere(target) {
        load();
        view.url = target;
    }

    // The sort lands after the listing: the selection is made again when the
    // rows move, until the timer ends it.
    function showItems(urls) {
        load();
        pendingSelect = urls;
        if (!view.folder.loading) {
            view.selectUrls(urls);
        }
        pendingTimer.restart();
    }

    Component.onCompleted: {
        if (!lazy) {
            load();
        }
    }
    onVisibleChanged: {
        if (visible) {
            load();
        }
    }

    Timer {
        id: pendingTimer
        interval: 1500
        onTriggered: page.pendingSelect = []
    }

    Connections {
        target: view.folder
        function onLoadingChanged() {
            if (!view.folder.loading && page.pendingSelect.length > 0) {
                view.selectUrls(page.pendingSelect);
            }
        }
        function onLayoutChanged() {
            if (page.pendingSelect.length > 0) {
                view.selectUrls(page.pendingSelect);
            }
        }
    }

    SearchController {
        id: tabSearch
        folder: view.folder
    }

    // F5 on a page reads it again.
    Connections {
        target: view.folder
        function onPageRefreshRequested() {
            if (pageLoader.item) {
                pageLoader.item.refresh();
            }
        }
    }

    // Leaving a page for a folder: the folder has the keyboard.
    onPageKindChanged: {
        if (pageKind.length === 0 && visible && page.activeFocus === false && view.visible) {
            Qt.callLater(() => view.forceActiveFocus());
        }
    }

    Component {
        id: homePage
        HomePage {
            actions: page.actions
            onNavigateRequested: (target, newTab) => newTab ? page.openInNewTab(target) : page.navigate(target)
        }
    }
    Component {
        id: networkPage
        NetworkPage {
            onNavigateRequested: (target, newTab) => newTab ? page.openInNewTab(target) : page.navigate(target)
            onConnectRequested: page.connectRequested()
        }
    }

    // The first pane's neighbour is on its right (its left in a mirrored layout).
    Rectangle {
        visible: page.leadingLine
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        width: 1
        z: 5
        color: TelamonStyle.separator
    }

    // Takes the keyboard (and so the toolbar) for this pane on a click or a key.
    onActiveFocusChanged: {
        if (activeFocus) {
            page.activated();
        }
    }
    TapHandler {
        acceptedButtons: Qt.AllButtons
        onPressedChanged: {
            if (pressed) {
                page.activated();
            }
        }
    }

    // The header, while the tab is split: an accent line on the active pane,
    // the pane's own path bar and its close button.
    Item {
        id: header
        visible: page.split
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: visible ? headerRow.implicitHeight + accentLine.height + Kirigami.Units.smallSpacing * 2 : 0
        z: 4

        Rectangle {
            id: accentLine
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            height: 3
            color: page.paneActive ? TelamonStyle.accent : "transparent"
            Accessible.ignored: true
        }
        RowLayout {
            id: headerRow
            anchors.top: accentLine.bottom
            anchors.topMargin: Kirigami.Units.smallSpacing
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            PathBar {
                id: headerBar
                Layout.fillWidth: true
                location: page.location
                showHidden: view.folder.showHidden
                actions: page.actions
                onNavigateRequested: target => page.navigate(target)
                onOpenInNewTabRequested: target => page.openInNewTab(target)
                onAddressAccepted: text => page.addressAccepted(text)
                onAddressEdited: page.addressEdited()
                onEditEnded: {
                    page.addressEdited();
                    page.focusContent();
                }
                onEditingChanged: {
                    if (editing) {
                        page.activated();
                    }
                }
            }
            ToolbarButton {
                symbol: Symbols.Close
                text: qsTr("Close Pane")
                toolTipText: qsTr("Close This Pane")
                focusable: true
                onClicked: page.closeRequested()
            }
        }
    }

    // The folder (or the page) under the header.
    Item {
        id: body
        anchors.top: header.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right

        Loader {
            id: pageLoader
            anchors.fill: parent
            active: page.pageKind.length > 0
            sourceComponent: page.pageKind === "network" ? networkPage : homePage
            onLoaded: {
                if (page.visible) {
                    page.focusContent();
                }
            }
        }
        // The Trash's header: the auto-empty setting.
        TrashBar {
            id: trashBar
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            visible: view.folder.inTrash && page.pageKind.length === 0
            height: visible ? implicitHeight : 0
        }

        FolderView {
            id: view
            anchors.fill: parent
            anchors.topMargin: trashBar.height
            focus: true
            actions: page.actions
            search: tabSearch
            onOpenLocationRequested: urls => page.openLocation(urls)
            onQuickLookRequested: page.quickLookRequested()
            onSearchCloseRequested: tabSearch.clear()
            onNavigateRequested: target => page.navigate(target)
            // A step between the columns: the folder, with the items selected in it.
            onColumnNavigateRequested: (target, select) => {
                page.navigate(target);
                if (select.length > 0) {
                    page.showItems(select);
                }
            }
            onOpenInNewTabRequested: target => page.openInNewTab(target)
            // KIO's own prompts apply (Run or open?, untrusted .desktop files).
            onOpenRequested: urls => page.actions.openUrls(urls)
            onRenameRequested: {
                // One item is renamed in place, several in Batch Rename (the window's).
                if (view.selectedUrls.length > 0 && view.folder.canWrite) {
                    page.actions.rename(view.selectedUrls);
                }
            }
            onContextMenuRequested: (urls, x, y) => page.contextMenuRequested(urls, x, y)
        }
    }
}
