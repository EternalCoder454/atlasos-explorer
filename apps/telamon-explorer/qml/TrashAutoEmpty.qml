pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// "Empty items older than [30] days": the switch and the number of days of
// the Trash setting (off by default). It sits in the Trash's header and in the
// View menu's Trash dialog until the Settings window has it. Turning it on
// asks first (TrashLogic); the controls always show what is kept, so an
// answer of No puts them back.
RowLayout {
    id: row

    spacing: TelamonStyle.spacingLarge

    TelamonSwitch {
        id: toggle
        checked: TrashLogic.autoEmpty
        Accessible.name: qsTr("Empty Trash items older than a number of days")
        onToggled: {
            TrashLogic.requestAutoEmpty(checked);
            // Kept as the setting says until the answer is in.
            checked = Qt.binding(() => TrashLogic.autoEmpty);
        }
    }
    Text {
        text: qsTr("Empty items older than")
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.textColor
        Accessible.ignored: true
        TapHandler {
            onTapped: {
                TrashLogic.requestAutoEmpty(!TrashLogic.autoEmpty);
            }
        }
    }
    TelamonSpinBox {
        id: days
        from: TrashLogic.minDays
        to: TrashLogic.maxDays
        editable: true
        value: TrashLogic.days
        Accessible.name: qsTr("Days")
        onValueModified: {
            TrashLogic.requestDays(value);
            value = Qt.binding(() => TrashLogic.days);
        }
    }
    Text {
        text: TrashLogic.days === 1 ? qsTr("day") : qsTr("days")
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.textColor
        Accessible.ignored: true
    }
}
