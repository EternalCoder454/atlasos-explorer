pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Asks for the name of a rename, a new folder or a new file, in a Telamon.Ui
// dialog. The name is checked as it is typed by FileActions (the core's rules
// and the folder's items): a refusal shows under the field and keeps the
// button off, a warning (a hidden character, a leading space) shows and the
// button then says "Use This Name". The work itself is queued by
// FileActions.acceptName, like any other.
TelamonDialog {
    id: dialog

    // The window's FileActions.
    required property var actions
    // What FileActions.namePromptRequested sent.
    property var request: ({})
    property var check: ({
            "ok": true,
            "text": ""
        })
    readonly property bool refused: !check.ok
    readonly property bool warned: check.ok && check.text.length > 0

    title: request.title ?? ""
    preferredWidth: Kirigami.Units.gridUnit * 26

    function ask(req) {
        request = req;
        field.text = req.initial ?? "";
        evaluate();
        open();
    }
    function evaluate() {
        check = actions.checkName(field.text, request);
    }
    function commit() {
        if (!check.ok) {
            return;
        }
        // The dialog closes before the work is queued: the view has the keyboard back.
        const req = request;
        const name = field.text;
        close();
        actions.acceptName(req, name);
    }

    onOpened: {
        field.forceActiveFocus();
        // A file's name without its extension is what is usually changed.
        const text = field.text;
        const dot = text.lastIndexOf(".");
        field.select(0, request.isDir !== true && dot > 0 ? dot : text.length);
    }

    footerContent: [
        SecondaryButton {
            text: qsTr("Cancel")
            onClicked: dialog.close()
        },
        PrimaryButton {
            text: dialog.warned ? qsTr("Use This Name") : (dialog.request.okText ?? qsTr("OK"))
            enabled: dialog.check.ok && field.text.length > 0
            onClicked: dialog.commit()
        }
    ]

    Text {
        Layout.fillWidth: true
        text: dialog.request.label ?? ""
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: TelamonStyle.textMuted
        elide: Text.ElideRight
    }
    TelamonTextField {
        id: field
        Layout.fillWidth: true
        maximumLength: 255
        errorText: dialog.refused ? dialog.check.text : ""
        onTextChanged: dialog.evaluate()
        onAccepted: dialog.commit()
        Accessible.name: dialog.request.label ?? ""
    }
    Text {
        Layout.fillWidth: true
        visible: dialog.warned
        text: dialog.check.text
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: Kirigami.Theme.neutralTextColor
        Accessible.role: Accessible.AlertMessage
        Accessible.name: text
    }
}
