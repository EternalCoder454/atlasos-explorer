pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Files' Network: the computers found on the local network (Avahi, and the
// SMB worker's own browsing), with a spinner and Stop while it looks, and the
// servers connected to before. Nothing is stored here. A computer that
// switches on later is added while the page is open.
FocusScope {
    id: page

    // A server to show in this tab, or (newTab) in a tab behind it.
    signal navigateRequested(url target, bool newTab)
    // "Connect to Server…" was chosen.
    signal connectRequested

    // F5 looks again.
    function refresh() {
        net.start();
    }

    Component.onCompleted: net.start()
    onVisibleChanged: {
        // A page that was away is brought up to date, once it has been a while.
        if (visible && !net.scanning && net.count === 0) {
            net.start();
        }
    }

    NetworkModel {
        id: net
    }

    // The servers connected to before; read again when they change.
    property var recents: ServerLogic.recents()
    Connections {
        target: ServerLogic
        function onRecentChanged() {
            page.recents = ServerLogic.recents();
        }
    }

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

            RowLayout {
                Layout.fillWidth: true
                spacing: TelamonStyle.spacingLarge
                Text {
                    text: qsTr("Network")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody * 1.6
                    font.bold: true
                    color: Kirigami.Theme.textColor
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                Item {
                    Layout.fillWidth: true
                }
                SecondaryButton {
                    text: qsTr("Connect to Server…")
                    onClicked: page.connectRequested()
                }
            }

            // While it looks: a spinner, and Stop.
            RowLayout {
                id: scanRow
                Layout.fillWidth: true
                visible: net.scanning
                spacing: TelamonStyle.spacingLarge
                TelamonSpinner {
                    running: net.scanning
                }
                Text {
                    Layout.fillWidth: true
                    text: qsTr("Looking for computers…")
                    textFormat: Text.PlainText
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    color: TelamonStyle.textMuted
                    Accessible.role: Accessible.StaticText
                    Accessible.name: text
                }
                SecondaryButton {
                    id: stopButton
                    text: qsTr("Stop")
                    onClicked: net.stop()
                }
            }

            TelamonExpandableSection {
                id: computers
                title: qsTr("Computers")
                expanded: true
                visible: net.count > 0
                Accessible.name: title
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacingSmall
                    Repeater {
                        model: net
                        delegate: HomeTile {
                            id: computer
                            required property string computerName
                            required property url computerUrl
                            required property string computerKind
                            required property string computerIcon
                            Layout.fillWidth: true
                            row: true
                            iconName: computer.computerIcon
                            text: computer.computerName
                            subtitle: computer.computerKind
                            onClicked: page.navigateRequested(computer.computerUrl, TabLogic.controlHeld())
                            onOpenInNewTab: page.navigateRequested(computer.computerUrl, true)
                        }
                    }
                }
            }

            // Nothing found, and not looking any more.
            TelamonEmptyState {
                Layout.fillWidth: true
                visible: net.count === 0 && !net.scanning
                symbol: Symbols.Lan
                title: qsTr("No Computers Found")
                text: net.discoveryAvailable ? qsTr("No computers on this network are sharing files that Files can open. Use Connect to Server to type an address.") : qsTr("Files can't look for computers on this system. Use Connect to Server to type an address.")
                actionText: qsTr("Look Again")
                onTriggered: net.start()
            }

            TelamonExpandableSection {
                id: recentServers
                title: qsTr("Recent Servers")
                expanded: true
                visible: page.recents.length > 0
                Accessible.name: title
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: TelamonStyle.spacingSmall
                    Repeater {
                        model: page.recents
                        delegate: HomeTile {
                            id: server
                            required property var modelData
                            Layout.fillWidth: true
                            row: true
                            iconName: "folder-remote"
                            text: server.modelData.label
                            subtitle: ServerLogic.securityNote(server.modelData.url)
                            onClicked: page.navigateRequested(server.modelData.url, TabLogic.controlHeld())
                            onOpenInNewTab: page.navigateRequested(server.modelData.url, true)
                        }
                    }
                }
            }
        }
    }
}
