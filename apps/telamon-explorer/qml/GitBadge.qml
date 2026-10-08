import QtQuick
import Telamon.Ui

// The Git status badge of an item (Settings > View > Show Git status): a small
// round mark with a letter, M modified, N new, I ignored, C in conflict. The
// letter makes it readable without colour, and under high contrast it has a
// border of the text colour. It is decoration for the eyes: the state is said
// in the row's accessible description ("Git: modified").
Rectangle {
    id: badge

    // 0 none, 1 modified, 2 new, 3 ignored, 4 in conflict (FolderModel.gitBadge).
    property int code: 0
    // The width of the badge.
    property real dot: 14

    visible: code > 0
    width: dot
    height: dot
    radius: dot / 2
    color: {
        switch (code) {
        case 1:
            return TelamonStyle.warning;
        case 2:
            return TelamonStyle.success;
        case 4:
            return TelamonStyle.error;
        default:
            return TelamonStyle.textMuted;
        }
    }
    border.width: TelamonStyle.highContrast ? 2 : 1
    border.color: TelamonStyle.highContrast ? TelamonStyle.text : Qt.alpha(TelamonStyle.base, 0.9)
    // Decoration: the state is in the row's accessible description.
    Accessible.ignored: true

    Text {
        anchors.centerIn: parent
        text: badge.code === 1 ? "M" : badge.code === 2 ? "N" : badge.code === 3 ? "I" : badge.code === 4 ? "C" : ""
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.bold: true
        font.pixelSize: Math.max(7, Math.round(badge.dot * 0.62))
        color: TelamonStyle.base
    }
}
