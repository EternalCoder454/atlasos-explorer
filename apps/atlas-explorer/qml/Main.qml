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

    // The place shown, a key of `places`.
    property string place: "home"
    // What the last launch asked to open, shown until the views exist.
    property string launchText

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
                root.launchText = locations.join("\n");
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
                    onClicked: root.place = modelData.key
                }
            }
        }

        Rectangle {
            Layout.fillHeight: true
            implicitWidth: 1
            color: Qt.alpha(Kirigami.Theme.textColor, 0.12)
        }

        PlaceholderPage {
            Layout.fillWidth: true
            Layout.fillHeight: true
            title: root.places.find(p => p.key === root.place)?.text ?? ""
            heading: root.launchText.length > 0 ? qsTr("Opening Locations") : qsTr("Not Built Yet")
            text: root.launchText.length > 0 ? root.launchText : qsTr("Folders and files arrive with the first usable version.")
        }
    }
}
