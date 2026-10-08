pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Connect to Server (Ctrl+Shift+K): protocol, server, folder and user. The
// address is made by the core from these fields (a field can't add a user, a
// password, a port or another host to it), and there is no password field:
// KIO asks for the password when the server wants one, and Files never keeps
// it. The servers connected to before are listed (an address with a password
// is never in the list). FTP and plain WebDAV say they are not encrypted.
TelamonDialog {
    id: dialog

    // The server to connect to (a URL), to show in this tab.
    signal connectRequested(url target)
    // The server to keep as a place in the sidebar.
    signal addToSidebarRequested(url target)

    title: qsTr("Connect to Server")
    preferredWidth: Kirigami.Units.gridUnit * 32

    property var protocols: ServerLogic.protocols()
    property var recents: ServerLogic.recents()
    readonly property int protocolCode: protocols.length > 0 && protocolBox.currentIndex >= 0 ? protocols[protocolBox.currentIndex].code : 0
    readonly property bool encrypted: protocols.length > 0 && protocolBox.currentIndex >= 0 ? protocols[protocolBox.currentIndex].encrypted : true
    // {ok, text}: the address, or why there isn't one.
    readonly property var built: ServerLogic.build(protocolCode, serverField.text, folderField.text, userField.text)
    // A refusal shows once something was typed.
    readonly property bool refused: !built.ok && (serverField.text.length > 0 || folderField.text.length > 0 || userField.text.length > 0)
    readonly property bool ready: built.ok

    function ask() {
        protocols = ServerLogic.protocols();
        recents = ServerLogic.recents();
        // Always a clean form: what was typed last time is in the recent list.
        protocolBox.currentIndex = 0;
        serverField.text = "";
        folderField.text = "";
        userField.text = "";
        open();
    }
    // A server of the list fills the fields.
    function fill(url) {
        const p = ServerLogic.parse(url);
        if (p.server === undefined) {
            return;
        }
        for (let i = 0; i < protocols.length; ++i) {
            if (protocols[i].code === p.protocol) {
                protocolBox.currentIndex = i;
            }
        }
        serverField.text = p.server;
        folderField.text = p.folder;
        userField.text = p.user;
        serverField.forceActiveFocus();
    }
    function connectNow() {
        if (!ready) {
            return;
        }
        const target = Qt.url(built.text);
        close();
        dialog.connectRequested(target);
    }
    function addNow() {
        if (!ready) {
            return;
        }
        const target = Qt.url(built.text);
        close();
        dialog.addToSidebarRequested(target);
    }

    Connections {
        target: ServerLogic
        function onRecentChanged() {
            dialog.recents = ServerLogic.recents();
        }
    }

    onOpened: serverField.forceActiveFocus()

    footerContent: [
        SecondaryButton {
            text: qsTr("Cancel")
            onClicked: dialog.close()
        },
        SecondaryButton {
            text: qsTr("Add to Sidebar")
            enabled: dialog.ready
            onClicked: dialog.addNow()
        },
        PrimaryButton {
            text: qsTr("Connect")
            enabled: dialog.ready
            onClicked: dialog.connectNow()
        }
    ]

    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: TelamonStyle.spacingLarge
        rowSpacing: TelamonStyle.spacing

        Text {
            text: qsTr("Protocol")
            textFormat: Text.PlainText
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
        }
        TelamonComboBox {
            id: protocolBox
            Layout.fillWidth: true
            model: dialog.protocols.map(p => p.label)
            currentIndex: 0
            Accessible.name: qsTr("Protocol")
        }

        Text {
            text: qsTr("Server")
            textFormat: Text.PlainText
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
        }
        TelamonTextField {
            id: serverField
            Layout.fillWidth: true
            maximumLength: 260
            placeholderText: qsTr("nas.local or 192.168.1.20")
            invalidText: ""
            Accessible.name: qsTr("Server")
            onAccepted: dialog.connectNow()
        }

        Text {
            text: qsTr("Folder")
            textFormat: Text.PlainText
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
        }
        TelamonTextField {
            id: folderField
            Layout.fillWidth: true
            maximumLength: 1024
            placeholderText: qsTr("Optional, like /srv/files or share/Photos")
            Accessible.name: qsTr("Folder")
            onAccepted: dialog.connectNow()
        }

        Text {
            text: qsTr("User")
            textFormat: Text.PlainText
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeBody
            color: TelamonStyle.textMuted
        }
        TelamonTextField {
            id: userField
            Layout.fillWidth: true
            maximumLength: 128
            placeholderText: qsTr("Optional")
            Accessible.name: qsTr("User")
            onAccepted: dialog.connectNow()
        }
    }

    // Why there is no address yet, in plain words (whichever field it is about).
    Text {
        Layout.fillWidth: true
        visible: dialog.refused
        text: dialog.built.text
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: Kirigami.Theme.negativeTextColor
        Accessible.role: Accessible.AlertMessage
        Accessible.name: text
    }
    Text {
        Layout.fillWidth: true
        text: qsTr("The server asks for the password when you connect. Files doesn't keep it.")
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: TelamonStyle.textMuted
    }
    // FTP and plain WebDAV send everything, a password too, for anyone on the path to read.
    Text {
        Layout.fillWidth: true
        visible: !dialog.encrypted
        text: qsTr("Not encrypted. Anyone on the network can read what you open or copy here. Use SFTP or WebDAV (Secure) when the server offers them.")
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: Kirigami.Theme.neutralTextColor
        Accessible.role: Accessible.AlertMessage
        Accessible.name: text
    }

    // The servers connected to before; a click fills the fields.
    Text {
        Layout.fillWidth: true
        Layout.topMargin: TelamonStyle.spacing
        visible: dialog.recents.length > 0
        text: qsTr("Recent Servers")
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        font.bold: true
        color: Kirigami.Theme.textColor
    }
    ColumnLayout {
        Layout.fillWidth: true
        visible: dialog.recents.length > 0
        spacing: TelamonStyle.spacingSmall
        Repeater {
            model: dialog.recents
            delegate: HomeTile {
                id: recent
                required property var modelData
                Layout.fillWidth: true
                row: true
                implicitHeight: Math.round(Kirigami.Units.gridUnit * 2.2)
                iconName: "folder-remote"
                text: recent.modelData.label
                subtitle: ServerLogic.securityNote(recent.modelData.url)
                onClicked: dialog.fill(recent.modelData.url)
            }
        }
        TextButton {
            Layout.alignment: Qt.AlignRight
            text: qsTr("Clear Recent Servers")
            onClicked: ServerLogic.clearRecents()
        }
    }
}
