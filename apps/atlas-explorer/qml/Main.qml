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
                root.navigate(locations[0]);
            }
        }
        function onRefused(text) {
            root.launchText = qsTr("Could not open:") + "\n" + text;
        }
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
                // The address bar replaces this label in the next part.
                Text {
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                    text: StandardPlaces.displayLocation(root.currentUrl)
                    color: Kirigami.Theme.textColor
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
                // Opening files, with the trust prompts, is wired by another part.
                onOpenRequested: urls => console.info("open requested:", urls.length)
            }
        }
    }
}
