pragma ComponentBehavior: Bound
import QtQuick

// One tab of the window: a folder with its own back and forward history, view
// mode, selection and scroll position (they live in the FolderView, which
// stays alive while the tab is hidden). The window owns the strip and the
// toolbar; this page answers navigate, goBack and goForward for the tab shown.
FocusScope {
    id: page

    // The window's FileActions (jobs, dialogs, context menus).
    required property var actions
    // The tab's number in the window; it does not change when tabs move.
    required property int tabId
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

    anchors.fill: parent

    property alias view: view
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

    FolderView {
        id: view
        anchors.fill: parent
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
            if (view.selectedUrls.length === 1 && view.folder.canWrite) {
                page.actions.rename(view.selectedUrls[0]);
            }
        }
        onContextMenuRequested: (urls, x, y) => page.contextMenuRequested(urls, x, y)
    }
}
