pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A question a job asks that is not a name conflict: a file that could not be
// copied (Retry, Skip, Skip All or Cancel), or an item that can't go to the
// Trash (delete it for good instead? Cancel is the default). Escape cancels.
TelamonDialog {
    id: dialog

    // The FileActions' OperationQueue.
    required property var queue
    readonly property var q: queue.question
    readonly property bool problem: q.type === "problem"
    readonly property bool mine: problem || q.type === "delete"

    title: !mine ? "" : problem ? qsTr("Couldn't Finish") : qsTr("Delete for Good?")
    preferredWidth: Kirigami.Units.gridUnit * 28
    showClose: false
    closePolicy: T.Popup.NoAutoClose
    Shortcut {
        sequence: "Escape"
        enabled: dialog.opened && dialog.mine
        onActivated: dialog.queue.answer({
            "answer": "cancel"
        })
    }

    Connections {
        target: dialog.queue
        function onQuestionChanged() {
            if (dialog.mine) {
                if (!dialog.opened) {
                    dialog.open();
                }
            } else {
                dialog.close();
            }
        }
    }
    onOpened: (dialog.problem ? retryButton : cancelButton).forceActiveFocus()

    footerContent: [
        SecondaryButton {
            id: cancelButton
            text: qsTr("Cancel")
            onClicked: dialog.queue.answer({
                "answer": "cancel"
            })
        },
        SecondaryButton {
            visible: dialog.problem && dialog.q.multiple
            text: qsTr("Skip All")
            onClicked: dialog.queue.answer({
                "answer": "skipAll"
            })
        },
        SecondaryButton {
            visible: dialog.problem
            text: qsTr("Skip")
            onClicked: dialog.queue.answer({
                "answer": "skip"
            })
        },
        PrimaryButton {
            id: retryButton
            visible: dialog.problem && dialog.q.canRetry
            text: qsTr("Retry")
            onClicked: dialog.queue.answer({
                "answer": "retry"
            })
        },
        TelamonButton {
            visible: !dialog.problem
            variant: TelamonButton.Destructive
            text: qsTr("Delete")
            onClicked: dialog.queue.answer({
                "answer": "yes"
            })
        }
    ]

    Text {
        Layout.fillWidth: true
        text: dialog.mine ? dialog.q.text : ""
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.textColor
        textFormat: Text.PlainText
    }
}
