import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One thing on the Home page: an icon, a name and a line under it (where it
// is, or when it was used), as a button. A click or Return opens it; a
// Ctrl+click or a middle click opens a folder in a tab behind. Names and
// paths come made safe to show, and are drawn as plain text.
T.AbstractButton {
    id: tile

    property string iconName
    property string subtitle
    // A wide row (Recent files) or a card in a flow (folders).
    property bool row: false

    // Open in a new background tab (Ctrl+click, middle click).
    signal openInNewTab

    implicitWidth: row ? Kirigami.Units.gridUnit * 30 : Kirigami.Units.gridUnit * 13
    implicitHeight: Math.round(Kirigami.Units.gridUnit * 2.6)
    leftPadding: TelamonStyle.spacingLarge
    rightPadding: TelamonStyle.spacingLarge
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.description: subtitle

    Keys.onReturnPressed: event => {
        if (!event.isAutoRepeat) {
            tile.clicked();
        }
    }
    Keys.onEnterPressed: event => {
        if (!event.isAutoRepeat) {
            tile.clicked();
        }
    }
    TapHandler {
        acceptedButtons: Qt.MiddleButton
        onTapped: tile.openInNewTab()
    }

    background: Rectangle {
        radius: TelamonStyle.radius
        color: tile.down ? TelamonStyle.pressed : (tile.hovered ? TelamonStyle.hover : Qt.alpha(Kirigami.Theme.textColor, 0.04))
        border.width: 1
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.1)
        TelamonFocusRing {
            radius: parent.radius
            shown: tile.visualFocus
        }
    }
    contentItem: RowLayout {
        spacing: TelamonStyle.spacingLarge
        Kirigami.Icon {
            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            source: tile.iconName
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Text {
                Layout.fillWidth: true
                text: tile.text
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
                Accessible.ignored: true
            }
            Text {
                Layout.fillWidth: true
                visible: tile.subtitle.length > 0
                text: tile.subtitle
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
                Accessible.ignored: true
            }
        }
    }
}
