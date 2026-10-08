pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// One tab of the window: one pane (FilesPane: a folder with its own history,
// view, selection and search), or two side by side when the tab is split
// (F3). The tab answers for the pane that is active, the one with the
// keyboard: the window's toolbar, path bar, shortcuts and menus act on it. The
// split state belongs to the tab: it is kept with the tab when it is
// duplicated, closed and reopened, and saved with the tabs for the next start.
FocusScope {
    id: tab

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
    // The second pane, for a tab that comes back split ("": not split), and
    // which pane has the keyboard.
    property url startSplit
    property int startActive: 0
    // A restored background tab loads its folders when it is first shown, so a
    // session of many tabs (or of slow servers) doesn't list them all at start.
    property bool lazy: false
    // Items to select once the folder has loaded (ShowItems, --select).
    property var pendingSelect: []

    anchors.fill: parent

    // The panes, left to right (the first is the one the tab started with),
    // and the one that has the keyboard.
    property var panes: []
    property int activeIndex: 0
    readonly property bool split: panes.length > 1
    readonly property FilesPane pane: panes.length > 0 ? panes[Math.min(activeIndex, panes.length - 1)] : null
    // What the window asks of "the tab shown" is the active pane's.
    readonly property FolderView view: pane ? pane.view : null
    readonly property SearchController search: pane ? pane.search : null
    readonly property bool loaded: pane ? pane.loaded : false
    readonly property url location: pane ? pane.location : startUrl
    readonly property string title: pane ? pane.title : ""
    readonly property string toolTip: pane ? pane.toolTip : ""
    readonly property var backStack: pane ? pane.backStack : []
    readonly property var forwardStack: pane ? pane.forwardStack : []
    readonly property bool canGoBack: pane ? pane.canGoBack : false
    readonly property bool canGoForward: pane ? pane.canGoForward : false
    readonly property string pageKind: pane ? pane.pageKind : ""
    // The pane that does not have the keyboard (null when not split).
    readonly property FilesPane otherPane: split ? panes[1 - Math.min(activeIndex, 1)] : null
    // The path bar of the active pane's header.
    readonly property PathBar activeBar: pane ? pane.pathBar : null

    // A folder to open in a new background tab.
    signal openInNewTab(url target)
    // The user moved this tab to another folder (not a restore or a launch).
    signal navigated
    // Open File Location on search results: their URLs.
    signal openLocation(var urls)
    // Space in the view: Quick Look for the selected file.
    signal quickLookRequested
    // A right click or the Menu key: the items (none: the folder's background),
    // where in the coordinates of `anchor` (the view it came from).
    signal contextMenuRequested(var urls, real x, real y, Item anchor)
    // "Connect to Server…" was chosen on the Network page.
    signal connectRequested
    // Enter in a pane's own path bar, with the text; and a key that changed it.
    signal addressAccepted(string text)
    signal addressEdited
    // The panes or the one with the keyboard changed.
    signal paneLayoutChanged

    // The keyboard goes to what the tab shows: its folder, or its page.
    function focusContent() {
        if (pane) {
            pane.focusContent();
        }
    }

    function load() {
        for (const p of panes) {
            p.load();
        }
    }

    // Goes to `target`, remembering where it was.
    function navigate(target) {
        if (pane) {
            pane.navigate(target);
        }
    }
    function goBack() {
        if (pane) {
            pane.goBack();
        }
    }
    function goForward() {
        if (pane) {
            pane.goForward();
        }
    }
    function goBackBy(n) {
        if (pane) {
            pane.goBackBy(n);
        }
    }
    function goForwardBy(n) {
        if (pane) {
            pane.goForwardBy(n);
        }
    }
    function backPlaces() {
        return pane ? pane.backPlaces() : [];
    }
    function forwardPlaces() {
        return pane ? pane.forwardPlaces() : [];
    }
    // Shows `target` without a history entry: the tab was never used (a first
    // launch with a folder to show).
    function showHere(target) {
        if (pane) {
            pane.showHere(target);
        }
    }
    function showItems(urls) {
        if (pane) {
            pane.showItems(urls);
        }
    }
    // Every pane's view (a setting for all views reaches both panes of the tab).
    function allViews() {
        return panes.map(p => p.view);
    }

    // ---- Split view ----
    Component {
        id: paneComponent
        FilesPane {
            actions: tab.actions
        }
    }

    // Makes a pane showing `url`, in the row.
    function makePane(url, o) {
        const p = paneComponent.createObject(row, {
            "startUrl": url,
            "startViewMode": o.viewMode || "",
            "startBack": o.back || [],
            "startForward": o.forward || [],
            "lazy": !!o.lazy,
            "pendingSelect": []
        });
        if (!p) {
            return null;
        }
        p.openInNewTab.connect(u => tab.openInNewTab(u));
        p.navigated.connect(() => tab.navigated());
        p.openLocation.connect(urls => tab.openLocation(urls));
        p.quickLookRequested.connect(() => {
            tab.activate(p);
            tab.quickLookRequested();
        });
        p.contextMenuRequested.connect((urls, x, y) => {
            tab.activate(p);
            tab.contextMenuRequested(urls, x, y, p.view);
        });
        p.connectRequested.connect(() => tab.connectRequested());
        p.activated.connect(() => tab.activate(p));
        p.closeRequested.connect(() => tab.closePane(tab.panes.indexOf(p)));
        p.addressAccepted.connect(text => tab.addressAccepted(text));
        p.addressEdited.connect(() => tab.addressEdited());
        return p;
    }

    // Tells every pane whether it is alone, first and active.
    function syncPanes() {
        for (let i = 0; i < panes.length; ++i) {
            panes[i].split = panes.length > 1;
            panes[i].paneActive = panes.length > 1 && i === Math.min(activeIndex, panes.length - 1);
            panes[i].leadingLine = i > 0;
        }
        paneLayoutChanged();
    }

    // `p` took the keyboard or a click: it is the pane the window acts on.
    function activate(p) {
        const i = panes.indexOf(p);
        if (i >= 0 && i !== activeIndex) {
            activeIndex = i;
            syncPanes();
        }
    }

    // Splits the tab: a second pane, showing `url` (the folder of the active
    // pane when none), takes the keyboard. Returns the pane, or null.
    function openSplit(url) {
        if (panes.length > 1 || !pane) {
            return panes.length > 1 ? panes[1] : null;
        }
        const here = pane;
        const p = makePane(url && url.toString().length > 0 ? url : here.location, {
            "viewMode": here.view.viewMode
        });
        if (!p) {
            return null;
        }
        panes = panes.concat([p]);
        activeIndex = 1;
        syncPanes();
        if (tab.visible) {
            p.load();
            p.focusContent();
        }
        return p;
    }

    // Closes pane `i`; the one left is the tab's pane again and has the keyboard.
    function closePane(i) {
        if (panes.length < 2 || i < 0 || i >= panes.length) {
            return;
        }
        const gone = panes[i];
        panes = panes.filter((_, k) => k !== i);
        activeIndex = 0;
        syncPanes();
        gone.visible = false;
        gone.destroy();
        if (tab.visible) {
            focusContent();
        }
    }

    // F3 and the toolbar's button: splits the tab, or closes the pane that
    // does not have the keyboard.
    function toggleSplit() {
        if (split) {
            closePane(1 - Math.min(activeIndex, 1));
        } else {
            openSplit();
        }
    }

    // The location of the second pane, for the tab's session ("" when alone).
    function splitLocation() {
        return split ? panes[1].location : Qt.url("");
    }

    Component.onCompleted: {
        const first = makePane(startUrl, {
            "viewMode": startViewMode,
            "back": startBack,
            "forward": startForward,
            "lazy": lazy
        });
        if (!first) {
            return;
        }
        first.pendingSelect = pendingSelect;
        const list = [first];
        if (startSplit.toString().length > 0) {
            const second = makePane(startSplit, {
                "lazy": lazy
            });
            if (second) {
                list.push(second);
            }
        }
        panes = list;
        activeIndex = list.length > 1 ? Math.max(0, Math.min(startActive, 1)) : 0;
        syncPanes();
    }

    RowLayout {
        id: row
        anchors.fill: parent
        spacing: 0
    }
}
