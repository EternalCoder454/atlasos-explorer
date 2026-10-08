pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Adds or changes a custom action (Settings > Context Menu and Actions): a name,
// the program (chosen with the file picker, or typed as a command name),
// its arguments with placeholders, the file types it is for and "ask first".
// The core checks it as it is typed (ActionsLogic.problem): the program must
// exist and be an executable file, the arguments must make sense, and a
// shell is refused. Nothing runs here.
TelamonDialog {
    id: dlg

    // 0 for a new action.
    property int actionId: 0
    readonly property string problem: ActionsLogic.problem(nameField.text, programField.path, argsField.text, typesField.text, askBox.checked)
    // The first try to save shows the problem; before it, a blank form says nothing.
    property bool tried: false

    title: actionId === 0 ? qsTr("Add Action") : qsTr("Edit Action")
    preferredWidth: Kirigami.Units.gridUnit * 34

    function openFor(id) {
        actionId = id;
        tried = false;
        const a = id === 0 ? {} : ActionsLogic.get(id);
        nameField.text = a.name ?? "";
        programField.path = a.program ?? "";
        argsField.text = a.args ?? "";
        typesField.text = a.types ?? "";
        askBox.checked = a.ask ?? true;
        open();
    }
    function save() {
        tried = true;
        if (problem.length > 0) {
            return;
        }
        const r = actionId === 0 ? ActionsLogic.add(nameField.text, programField.path, argsField.text, typesField.text, askBox.checked) : ActionsLogic.update(actionId, nameField.text, programField.path, argsField.text, typesField.text, askBox.checked);
        if (r === 0) {
            close();
        }
    }
    onOpened: nameField.forceActiveFocus()

    footerContent: [
        SecondaryButton {
            text: qsTr("Cancel")
            onClicked: dlg.close()
        },
        PrimaryButton {
            text: qsTr("Save")
            onClicked: dlg.save()
        }
    ]

    TelamonFormEntry {
        Layout.fillWidth: true
        label: qsTr("Name")
        help: qsTr("What the entry says in the menu.")
        TelamonTextField {
            id: nameField
            maximumLength: ActionsLogic.maxNameLength
            placeholderText: qsTr("Resize Pictures")
            onAccepted: dlg.save()
        }
    }
    TelamonFormEntry {
        Layout.fillWidth: true
        label: qsTr("Program")
        help: qsTr("Choose the program, or type its name, like convert. A shell can't be used.")
        TelamonFileField {
            id: programField
            placeholderText: qsTr("Choose a program or type its name")
            title: qsTr("Choose a Program")
            nameFilters: [qsTr("All files (*)")]
        }
    }
    TelamonFormEntry {
        Layout.fillWidth: true
        label: qsTr("Arguments")
        help: qsTr("Quote words with spaces. %f is a file, %F all the files, %u a URL, %U all the URLs, %d the folder.")
        TelamonTextField {
            id: argsField
            maximumLength: ActionsLogic.maxArgsLength
            placeholderText: qsTr("-resize 50% %f")
            onAccepted: dlg.save()
        }
    }
    TelamonFormEntry {
        Layout.fillWidth: true
        label: qsTr("File types")
        help: qsTr("Types of files it is for, like image/* application/pdf. Empty means every item.")
        TelamonTextField {
            id: typesField
            placeholderText: qsTr("image/*")
            onAccepted: dlg.save()
        }
    }
    TelamonFormEntry {
        Layout.fillWidth: true
        label: qsTr("Ask first")
        help: qsTr("Show the command and ask before it runs.")
        TelamonSwitch {
            id: askBox
            checked: true
        }
    }
    Text {
        Layout.fillWidth: true
        visible: dlg.tried && dlg.problem.length > 0
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
        text: dlg.problem
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.negativeTextColor
        Accessible.role: Accessible.AlertMessage
        Accessible.name: text
    }
}
