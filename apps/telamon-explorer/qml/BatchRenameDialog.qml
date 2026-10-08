pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Batch Rename: the names of the selected items made by one rule (find and
// replace, a number, a change of case, text added), with the names before and
// after listed as the rule is set. An item whose new name can't be used (not
// a name, the same as another item's, a name a selected item has now, a file
// that is there already) is marked with the reason, and Apply stays off until
// none is left. Apply renames them all as one step that Undo takes back.
//
// The names come from FileActions.batchPreview (the core's rules); Apply asks
// again, so the folder changing under the dialog can't slip a bad name in.
TelamonDialog {
    id: dialog

    // The window's FileActions.
    required property var actions
    // The items being renamed.
    property var urls: []
    // What the core made of the rule: see FileActions.batchPreview.
    property var plan: ({
            "valid": false,
            "canApply": false,
            "changed": 0,
            "blocked": 0,
            "problem": "",
            "rows": []
        })
    // 0 find and replace, 1 add number, 2 change case, 3 add text.
    property int mode: 0

    title: qsTr("Batch Rename")
    preferredWidth: Kirigami.Units.gridUnit * 42

    readonly property var modes: ["replace", "number", "case", "text"]
    readonly property var cases: ["lower", "upper", "title", "sentence"]

    function ask(list) {
        urls = list;
        mode = 0;
        modeBar.currentIndex = 0;
        findField.text = "";
        withField.text = "";
        matchCase.checked = false;
        regexBox.checked = false;
        startBox.value = 1;
        stepBox.value = 1;
        paddingBox.value = 3;
        numberPlace.currentIndex = 1;
        separatorField.text = " ";
        casePick.currentIndex = 0;
        addField.text = "";
        addPlace.currentIndex = 1;
        refresh();
        open();
    }
    function spec() {
        return {
            "mode": modes[mode],
            "find": findField.text,
            "replace": withField.text,
            "matchCase": matchCase.checked,
            "regex": regexBox.checked,
            "start": startBox.value,
            "step": stepBox.value,
            "padding": paddingBox.value,
            "atEnd": mode === 1 ? numberPlace.currentIndex === 1 : addPlace.currentIndex === 1,
            "separator": separatorField.text,
            "text": addField.text,
            "caseMode": cases[casePick.currentIndex]
        };
    }
    function refresh() {
        plan = actions.batchPreview(urls, spec());
    }
    function apply() {
        if (!plan.canApply) {
            return;
        }
        const list = urls;
        const rule = spec();
        // The dialog closes first: the view has the keyboard back.
        close();
        actions.batchApply(list, rule);
    }

    readonly property string summary: {
        if (!plan.valid) {
            return qsTr("These items can't be renamed together. Select them again.");
        }
        if (plan.problem.length > 0) {
            return plan.problem;
        }
        if (plan.blocked > 0) {
            return (plan.blocked === 1 ? qsTr("1 item can't get the name the rule gives.") : qsTr("%1 items can't get the name the rule gives.").arg(plan.blocked)) + " " + qsTr("Change the rule to go on.");
        }
        if (plan.changed === 0) {
            return qsTr("No names change yet.");
        }
        return plan.changed === 1 ? qsTr("1 item will be renamed.") : qsTr("%1 items will be renamed.").arg(plan.changed);
    }
    readonly property bool trouble: !plan.valid || plan.problem.length > 0 || plan.blocked > 0

    onOpened: Qt.callLater(() => {
        if (dialog.mode === 0) {
            findField.forceActiveFocus();
        }
    })

    footerContent: [
        SecondaryButton {
            text: qsTr("Cancel")
            onClicked: dialog.close()
        },
        PrimaryButton {
            text: qsTr("Rename")
            enabled: dialog.plan.canApply === true
            onClicked: dialog.apply()
        }
    ]

    Text {
        Layout.fillWidth: true
        text: dialog.urls.length === 1 ? qsTr("Rename 1 item by one rule.") : qsTr("Rename %1 items by one rule.").arg(dialog.urls.length)
        textFormat: Text.PlainText
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: TelamonStyle.textMuted
        elide: Text.ElideRight
    }

    TelamonSegmentedControl {
        id: modeBar
        Layout.fillWidth: true
        model: [qsTr("Find and Replace"), qsTr("Add Number"), qsTr("Change Case"), qsTr("Add Text")]
        Accessible.name: qsTr("Kind of rename")
        onActivated: index => {
            dialog.mode = index;
            dialog.refresh();
        }
    }

    // ---- The rule ----
    StackLayout {
        Layout.fillWidth: true
        currentIndex: dialog.mode
        // As tall as the tallest page: the dialog doesn't change size with the kind.
        Layout.preferredHeight: Math.max(pageReplace.implicitHeight, pageNumber.implicitHeight, pageCase.implicitHeight, pageText.implicitHeight)

        ColumnLayout {
            id: pageReplace
            spacing: Kirigami.Units.smallSpacing
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                TelamonTextField {
                    id: findField
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    placeholderText: qsTr("Find")
                    maximumLength: 512
                    onTextChanged: dialog.refresh()
                    onAccepted: dialog.apply()
                    Accessible.name: qsTr("Text to find")
                }
                TelamonTextField {
                    id: withField
                    Layout.fillWidth: true
                    Layout.preferredWidth: 1
                    placeholderText: regexBox.checked ? qsTr("Replace with (${1} is the first group)") : qsTr("Replace with")
                    maximumLength: 255
                    onTextChanged: dialog.refresh()
                    onAccepted: dialog.apply()
                    Accessible.name: qsTr("Replacement")
                }
            }
            RowLayout {
                spacing: Kirigami.Units.gridUnit
                TelamonCheckBox {
                    id: matchCase
                    text: qsTr("Match case")
                    onToggled: dialog.refresh()
                }
                TelamonCheckBox {
                    id: regexBox
                    text: qsTr("Regular expression")
                    onToggled: dialog.refresh()
                }
            }
        }

        ColumnLayout {
            id: pageNumber
            spacing: Kirigami.Units.smallSpacing
            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.gridUnit
                ColumnLayout {
                    spacing: 2
                    Text {
                        text: qsTr("Start at")
                        textFormat: Text.PlainText
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: TelamonStyle.textMuted
                    }
                    TelamonSpinBox {
                        id: startBox
                        from: 0
                        to: 999999999
                        value: 1
                        editable: true
                        onValueModified: dialog.refresh()
                        Accessible.name: qsTr("First number")
                    }
                }
                ColumnLayout {
                    spacing: 2
                    Text {
                        text: qsTr("Step")
                        textFormat: Text.PlainText
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: TelamonStyle.textMuted
                    }
                    TelamonSpinBox {
                        id: stepBox
                        from: 0
                        to: 1000000
                        value: 1
                        editable: true
                        onValueModified: dialog.refresh()
                        Accessible.name: qsTr("Step between numbers")
                    }
                }
                ColumnLayout {
                    spacing: 2
                    Text {
                        text: qsTr("Digits")
                        textFormat: Text.PlainText
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: TelamonStyle.textMuted
                    }
                    TelamonSpinBox {
                        id: paddingBox
                        from: 0
                        to: 12
                        value: 3
                        editable: true
                        onValueModified: dialog.refresh()
                        Accessible.name: qsTr("Digits, padded with zeros")
                    }
                }
                ColumnLayout {
                    spacing: 2
                    Text {
                        text: qsTr("Between")
                        textFormat: Text.PlainText
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: TelamonStyle.textMuted
                    }
                    TelamonTextField {
                        id: separatorField
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 5
                        text: " "
                        maximumLength: 16
                        onTextChanged: dialog.refresh()
                        Accessible.name: qsTr("Text between the name and the number")
                    }
                }
            }
            TelamonSegmentedControl {
                id: numberPlace
                model: [qsTr("Before the name"), qsTr("After the name")]
                currentIndex: 1
                Accessible.name: qsTr("Where the number goes")
                onActivated: dialog.refresh()
            }
            Text {
                text: qsTr("A file's extension stays at the end.")
                textFormat: Text.PlainText
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
            }
        }

        ColumnLayout {
            id: pageCase
            spacing: Kirigami.Units.smallSpacing
            TelamonSegmentedControl {
                id: casePick
                model: [qsTr("lower case"), qsTr("UPPER CASE"), qsTr("Title Case"), qsTr("Sentence case")]
                Accessible.name: qsTr("Letter case")
                onActivated: dialog.refresh()
            }
            Text {
                text: qsTr("A file's extension is left as it is.")
                textFormat: Text.PlainText
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
            }
        }

        ColumnLayout {
            id: pageText
            spacing: Kirigami.Units.smallSpacing
            TelamonTextField {
                id: addField
                Layout.fillWidth: true
                placeholderText: qsTr("Text to add")
                maximumLength: 255
                onTextChanged: dialog.refresh()
                onAccepted: dialog.apply()
                Accessible.name: qsTr("Text to add")
            }
            TelamonSegmentedControl {
                id: addPlace
                model: [qsTr("Before the name"), qsTr("After the name")]
                currentIndex: 1
                Accessible.name: qsTr("Where the text goes")
                onActivated: dialog.refresh()
            }
            Text {
                text: qsTr("A file's extension stays at the end.")
                textFormat: Text.PlainText
                font.pointSize: TelamonStyle.fontSizeCaption
                color: TelamonStyle.textMuted
            }
        }
    }

    // ---- The names, before and after ----
    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: Kirigami.Units.gridUnit * 14
        radius: TelamonStyle.radiusSmall
        color: Qt.alpha(Kirigami.Theme.textColor, 0.04)
        border.width: 1
        border.color: Qt.alpha(Kirigami.Theme.textColor, 0.15)
        clip: true

        ListView {
            id: list
            anchors.fill: parent
            anchors.margins: 1
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: dialog.plan.rows
            QQC2.ScrollBar.vertical: TelamonScrollBar {}
            Accessible.name: qsTr("The names before and after")
            Accessible.role: Accessible.List

            delegate: Item {
                id: entry
                required property var modelData
                required property int index
                readonly property int code: modelData.code
                readonly property bool bad: code >= 3
                width: list.width
                implicitHeight: rowLayout.implicitHeight + Kirigami.Units.smallSpacing * 2
                height: implicitHeight

                Rectangle {
                    anchors.fill: parent
                    color: entry.bad ? Qt.alpha(Kirigami.Theme.negativeTextColor, 0.1) : "transparent"
                }
                ColumnLayout {
                    id: rowLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: Kirigami.Units.largeSpacing
                    anchors.rightMargin: Kirigami.Units.largeSpacing
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.largeSpacing
                        Text {
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            text: entry.modelData.old
                            textFormat: Text.PlainText
                            elide: Text.ElideMiddle
                            color: entry.code === 0 ? Qt.alpha(Kirigami.Theme.textColor, 0.6) : Kirigami.Theme.textColor
                        }
                        Text {
                            text: "→"
                            textFormat: Text.PlainText
                            color: Qt.alpha(Kirigami.Theme.textColor, 0.5)
                        }
                        Text {
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            text: entry.code === 0 ? qsTr("No change") : entry.modelData.new
                            textFormat: Text.PlainText
                            elide: Text.ElideMiddle
                            font.bold: entry.code === 1 || entry.code === 2
                            color: entry.code === 0 ? Qt.alpha(Kirigami.Theme.textColor, 0.6) : (entry.bad ? Kirigami.Theme.negativeTextColor : (entry.code === 2 ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.textColor))
                            Accessible.name: entry.code === 0 ? qsTr("Not changed") : text
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: entry.modelData.text.length > 0
                        text: entry.modelData.text
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: entry.bad ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.neutralTextColor
                    }
                }
            }
        }
    }

    Text {
        Layout.fillWidth: true
        text: dialog.summary
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: TelamonStyle.fontFamily
        font.pointSize: TelamonStyle.fontSizeBody
        color: dialog.trouble ? Kirigami.Theme.negativeTextColor : TelamonStyle.textMuted
        Accessible.role: dialog.trouble ? Accessible.AlertMessage : Accessible.StaticText
        Accessible.name: text
    }
}
