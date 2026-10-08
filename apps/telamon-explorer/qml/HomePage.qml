pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Files' Home: three plain sections and nothing else. Pinned is the sidebar's
// own folders (and the way to the Home Folder), Recent files are the ones the
// system lists as recently used, Frequent folders are the ones this window
// went to often (counted on this computer only, with a Clear button). Each
// section folds, and remembers that. Nothing is recommended and nothing comes
// from the cloud.
FocusScope {
    id: page

    // The window's FileActions (opening files runs through its prompts).
    required property var actions

    // A folder to show in this tab, or (newTab) in a tab behind it.
    signal navigateRequested(url target, bool newTab)

    // F5 on the page reads the lists again.
    function refresh() {
        HomeLogic.refresh();
    }

    function openItem(item, newTab) {
        if (item.isDir) {
            page.navigateRequested(item.url, newTab);
        } else {
            page.actions.openUrls([item.url]);
        }
    }

    onVisibleChanged: {
        if (visible) {
            HomeLogic.refresh();
        }
    }
    Component.onCompleted: HomeLogic.refresh()
    // Tab walks the tiles; nothing here is a text field.
    activeFocusOnTab: true

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight + Kirigami.Units.gridUnit * 2
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        QQC2.ScrollBar.vertical: TelamonScrollBar {}

        ColumnLayout {
            id: column
            x: Kirigami.Units.gridUnit
            y: Kirigami.Units.gridUnit
            width: Math.min(flick.width - Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 60)
            spacing: Kirigami.Units.largeSpacing

            Text {
                text: qsTr("Home")
                textFormat: Text.PlainText
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody * 1.6
                font.bold: true
                color: Kirigami.Theme.textColor
                Accessible.role: Accessible.Heading
                Accessible.name: text
            }

            // ---- Pinned ----
            TelamonExpandableSection {
                id: pinned
                title: qsTr("Pinned")
                expanded: HomeLogic.sectionOpen("pinned")
                onToggled: open => HomeLogic.setSectionOpen("pinned", open)
                Accessible.name: title
                TelamonFlowLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacing
                    Repeater {
                        model: HomeLogic.pinned
                        delegate: HomeTile {
                            required property var modelData
                            iconName: modelData.iconName
                            text: modelData.name
                            subtitle: modelData.path
                            onClicked: page.openItem(modelData, TabLogic.controlHeld())
                            onOpenInNewTab: page.openItem(modelData, true)
                        }
                    }
                }
            }

            // ---- Recent files ----
            TelamonExpandableSection {
                id: recent
                title: qsTr("Recent Files")
                expanded: HomeLogic.sectionOpen("recent")
                onToggled: open => HomeLogic.setSectionOpen("recent", open)
                Text {
                    Layout.fillWidth: true
                    visible: HomeLogic.recent.length === 0
                    text: HomeLogic.loading ? qsTr("Looking…") : qsTr("Files you open will show up here.")
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacingSmall
                    Repeater {
                        model: HomeLogic.recent
                        delegate: HomeTile {
                            required property var modelData
                            Layout.fillWidth: true
                            row: true
                            iconName: modelData.iconName
                            text: modelData.name
                            subtitle: modelData.path.length > 0 ? modelData.path + " · " + modelData.tip : modelData.tip
                            onClicked: page.openItem(modelData, false)
                            // A file's folder, not the file, opens in a tab behind.
                            onOpenInNewTab: page.navigateRequested(StandardPlaces.parentUrl(modelData.url), true)
                        }
                    }
                }
            }

            // ---- Frequent folders ----
            Item {
                Layout.fillWidth: true
                implicitHeight: frequent.implicitHeight

                TelamonExpandableSection {
                    id: frequent
                    width: parent.width
                    title: qsTr("Frequent Folders")
                    expanded: HomeLogic.sectionOpen("frequent")
                    onToggled: open => HomeLogic.setSectionOpen("frequent", open)
                    Text {
                        Layout.fillWidth: true
                        visible: HomeLogic.frequent.length === 0
                        text: qsTr("Folders you go to often will show up here. Files counts them on this computer only.")
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeBody
                        color: TelamonStyle.textMuted
                    }
                    TelamonFlowLayout {
                        Layout.fillWidth: true
                        spacing: TelamonStyle.spacing
                        Repeater {
                            model: HomeLogic.frequent
                            delegate: HomeTile {
                                required property var modelData
                                iconName: modelData.iconName
                                text: modelData.name
                                subtitle: modelData.path
                                onClicked: page.openItem(modelData, TabLogic.controlHeld())
                                onOpenInNewTab: page.openItem(modelData, true)
                            }
                        }
                    }
                }
                // Clear forgets every count (and the list goes empty at once).
                TextButton {
                    id: clearButton
                    anchors.right: parent.right
                    anchors.rightMargin: Kirigami.Units.gridUnit * 2.5
                    y: Math.round((TelamonStyle.rowHeight - height) / 2)
                    text: qsTr("Clear")
                    visible: frequent.expanded && HomeLogic.counted > 0
                    Accessible.name: qsTr("Clear Frequent Folders")
                    onClicked: HomeLogic.clearFrequent()
                }
            }
        }
    }
}
