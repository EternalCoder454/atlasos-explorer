pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Templates as T
import org.kde.kirigami as Kirigami
import Telamon.Ui

// A name that is already taken at the destination: the file there and the one
// coming, side by side with their size, date and which is newer, and the
// answers that fit (a file: Replace, Skip, Keep Both with a name you can
// change; a folder: Merge or Skip). "Do this for all conflicts" answers the
// rest of the operation's the same way. Escape cancels the operation.
TelamonDialog {
    id: dialog

    // The FileActions' OperationQueue.
    required property var queue
    readonly property var q: queue.question
    readonly property bool mine: q.type === "conflict"
    readonly property var src: mine ? q.source : ({})
    readonly property var dst: mine ? q.dest : ({})
    // The name Keep Both would give, and what is wrong with it.
    property string newName: ""
    readonly property string nameProblem: mine && q.keepBoth ? queue.checkName(newName, q.existingName) : ""

    title: !mine ? "" : q.folders ? qsTr("Folder Already Exists") : qsTr("File Already Exists")
    preferredWidth: Kirigami.Units.gridUnit * 36
    showClose: false
    closePolicy: T.Popup.NoAutoClose
    Shortcut {
        sequence: "Escape"
        enabled: dialog.opened && dialog.mine
        onActivated: dialog.reply("cancel")
    }

    function reply(what) {
        queue.answer({
            "answer": what,
            "all": allCheck.checked,
            "name": newName
        });
    }

    // Opens for each conflict in turn; closes when there is none.
    Connections {
        target: dialog.queue
        function onQuestionChanged() {
            if (dialog.mine) {
                dialog.newName = dialog.q.suggested;
                allCheck.checked = false;
                if (!dialog.opened) {
                    dialog.open();
                }
            } else {
                dialog.close();
            }
        }
    }
    onOpened: {
        const d = dialog.q.defaultAnswer;
        const target = d === "keepBoth" ? keepButton : d === "merge" ? mergeButton : skipButton;
        target.forceActiveFocus();
    }

    // One of the two files.
    component FileCard: Rectangle {
        id: card
        required property var info
        required property string heading
        Layout.fillWidth: true
        Layout.preferredWidth: 1
        implicitHeight: content.implicitHeight + TelamonStyle.spacingLarge * 2
        radius: TelamonStyle.radius
        color: TelamonStyle.control
        border.width: 1
        border.color: card.info.newer ? TelamonStyle.accent : TelamonStyle.controlBorder

        ColumnLayout {
            id: content
            anchors.fill: parent
            anchors.margins: TelamonStyle.spacingLarge
            spacing: TelamonStyle.spacingSmall
            RowLayout {
                Layout.fillWidth: true
                Text {
                    Layout.fillWidth: true
                    text: card.heading
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeCaption
                    font.weight: Font.DemiBold
                    color: TelamonStyle.textMuted
                    textFormat: Text.PlainText
                }
                TelamonBadge {
                    visible: card.info.newer === true
                    text: qsTr("Newer")
                    type: "accent"
                }
            }
            Item {
                Layout.alignment: Qt.AlignHCenter
                implicitWidth: Kirigami.Units.gridUnit * 5
                implicitHeight: Kirigami.Units.gridUnit * 5
                Kirigami.Icon {
                    anchors.fill: parent
                    source: card.info.icon
                    visible: thumb.status !== Image.Ready
                }
                Image {
                    id: thumb
                    anchors.fill: parent
                    source: card.info.thumb
                    asynchronous: true
                    fillMode: Image.PreserveAspectFit
                    sourceSize: Qt.size(width * 2, height * 2)
                    visible: status === Image.Ready
                }
            }
            Text {
                Layout.fillWidth: true
                text: card.info.name
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideMiddle
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                font.weight: Font.Medium
                color: Kirigami.Theme.textColor
                textFormat: Text.PlainText
            }
            Text {
                Layout.fillWidth: true
                text: card.info.isDir ? qsTr("Folder") : card.info.sizeText
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
                textFormat: Text.PlainText
            }
            Text {
                Layout.fillWidth: true
                visible: card.info.dateText.length > 0
                text: card.info.dateText
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
                textFormat: Text.PlainText
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("In %1").arg(card.info.where)
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideMiddle
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
                textFormat: Text.PlainText
            }
        }
    }

    footerContent: [
        SecondaryButton {
            text: qsTr("Cancel")
            onClicked: dialog.reply("cancel")
        },
        SecondaryButton {
            id: skipButton
            visible: dialog.mine && dialog.q.skip
            text: qsTr("Skip")
            onClicked: dialog.reply("skip")
        },
        SecondaryButton {
            id: keepButton
            visible: dialog.mine && dialog.q.keepBoth
            enabled: dialog.nameProblem.length === 0
            text: qsTr("Keep Both")
            onClicked: dialog.reply("keepBoth")
        },
        TelamonButton {
            visible: dialog.mine && dialog.q.replace
            variant: TelamonButton.Destructive
            text: qsTr("Replace")
            onClicked: dialog.reply("replace")
        },
        PrimaryButton {
            id: mergeButton
            visible: dialog.mine && dialog.q.merge
            text: qsTr("Merge")
            onClicked: dialog.reply("merge")
        }
    ]

    Text {
        Layout.fillWidth: true
        text: !dialog.mine ? "" : dialog.q.folders ? qsTr("%1: a folder named “%2” is already in the destination.").arg(dialog.q.verb).arg(dialog.dst.name) : dialog.q.same ? qsTr("“%1” is already here. It can't replace itself.").arg(dialog.dst.name) : qsTr("%1: a file named “%2” is already in the destination.").arg(dialog.q.verb).arg(dialog.dst.name)
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: Kirigami.Theme.textColor
        textFormat: Text.PlainText
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: TelamonStyle.spacingLarge
        FileCard {
            heading: qsTr("Already here")
            info: dialog.dst
        }
        FileCard {
            heading: dialog.q.moving ? qsTr("Being moved") : qsTr("Being copied")
            info: dialog.src
        }
    }

    Text {
        Layout.fillWidth: true
        visible: dialog.mine && dialog.q.mismatch
        text: qsTr("A file and a folder can't replace each other.")
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeCaption
        color: TelamonStyle.textMuted
        textFormat: Text.PlainText
    }

    ColumnLayout {
        Layout.fillWidth: true
        visible: dialog.mine && dialog.q.keepBoth
        spacing: TelamonStyle.spacingSmall
        Text {
            text: qsTr("Keep both, naming the new one:")
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            color: TelamonStyle.textMuted
            textFormat: Text.PlainText
        }
        TelamonTextField {
            id: nameField
            Layout.fillWidth: true
            text: dialog.newName
            errorText: dialog.nameProblem
            onTextEdited: dialog.newName = text
            onAccepted: {
                if (dialog.nameProblem.length === 0) {
                    dialog.reply("keepBoth");
                }
            }
        }
    }

    TelamonCheckBox {
        id: allCheck
        visible: dialog.mine && dialog.q.multiple
        text: qsTr("Do this for all conflicts")
    }
}
