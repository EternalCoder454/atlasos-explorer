pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import Telamon.Ui

// One item's name, edited where it is shown (Details, Icons, Compact and
// Columns). It opens with the name without its extension selected. Enter
// renames; Escape leaves the name as it was; clicking away renames too, when
// the name can be used (a refused name, or one that wants a second look, is
// left as it was and Files says so). The name is checked as it is typed, by
// FileActions (the core's rules and the folder's items), and the reason shows
// under the field in plain words. A warning (a hidden character, a leading
// space) shows too, and Enter then needs pressing a second time.
//
// The FolderView owns what is being renamed (`renameUrl`) and this exists
// only for that item; it takes the keyboard when it appears and gives it back
// to the view when it is done.
FocusScope {
    id: ed

    // The FolderView.
    required property var fv
    // The item being renamed, and whether it is a folder.
    required property url itemUrl
    property bool isDir: false
    // The name is centered (the Icons view) rather than at the start.
    property bool centered: false

    readonly property var actions: fv.actions
    property var check: ({
            "ok": true,
            "text": ""
        })
    readonly property bool refused: !check.ok
    readonly property bool warned: check.ok && check.text.length > 0
    // Enter was pressed once on a name that has a warning.
    property bool confirmed: false
    property bool finished: false
    // The item this field was made for. `itemUrl` follows the row, which may
    // come to show another item (a reused row); what is renamed never does.
    property url target
    // The field has its first text (typing after that is the user's).
    property bool ready: false
    property string original: ""
    readonly property string message: refused ? check.text : (warned ? (confirmed ? check.text : check.text + " " + qsTr("Press Enter again to use this name.")) : "")

    implicitHeight: input.implicitHeight + Kirigami.Units.smallSpacing * 2

    function evaluate() {
        check = actions.checkName(input.text, {
                "mode": "rename",
                "url": target
            });
        confirmed = false;
    }

    // Ends the edit; with `commit`, renames to what is typed (when it differs).
    // `refocus`: the view takes the keyboard back (not when the user has just
    // put it somewhere else).
    function finish(commit, select, refocus = true) {
        if (finished) {
            return;
        }
        finished = true;
        const text = input.text;
        const url = target;
        const a = actions;
        const view = fv;
        view.endRename(refocus);
        if (commit && text !== original) {
            a.renameTo(url, text, select);
        }
    }

    // Enter.
    function accept() {
        if (text === original) {
            finish(false, false);
            return;
        }
        if (refused) {
            return;
        }
        if (warned && !confirmed) {
            confirmed = true;
            return;
        }
        finish(true, true);
    }
    readonly property string text: input.text

    // The keyboard went elsewhere, or the view was clicked: rename if the
    // name can simply be used, else leave it and say why.
    function leave(refocus = true) {
        if (finished) {
            return;
        }
        if (input.text === original) {
            finish(false, false, refocus);
        } else if (check.ok && !warned) {
            finish(true, false, refocus);
        } else {
            const why = check.text;
            finish(false, false, refocus);
            actions.tell(qsTr("The name wasn't changed. %1").arg(why));
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: TelamonStyle.radiusSmall
        color: Kirigami.Theme.backgroundColor
        border.width: 1
        border.color: ed.refused ? Kirigami.Theme.negativeTextColor : (ed.warned ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.highlightColor)
    }

    TextInput {
        id: input
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing * 2
        anchors.rightMargin: Kirigami.Units.smallSpacing * 2
        verticalAlignment: TextInput.AlignVCenter
        horizontalAlignment: ed.centered ? TextInput.AlignHCenter : TextInput.AlignLeft
        clip: true
        maximumLength: 255
        selectByMouse: true
        activeFocusOnPress: true
        color: Kirigami.Theme.textColor
        selectionColor: Kirigami.Theme.highlightColor
        selectedTextColor: Kirigami.Theme.highlightedTextColor
        focus: true
        // Typing keeps what is typed in the view, so the field comes back as it
        // was if the row is scrolled away and back.
        onTextChanged: {
            if (ed.ready) {
                ed.fv.renameText = text;
                ed.fv.renameDirty = true;
            }
            ed.evaluate();
        }
        Keys.onPressed: event => {
            switch (event.key) {
            case Qt.Key_Return:
            case Qt.Key_Enter:
                ed.accept();
                event.accepted = true;
                break;
            case Qt.Key_Escape:
                ed.finish(false, false);
                event.accepted = true;
                break;
            case Qt.Key_Up:
            case Qt.Key_Down:
            case Qt.Key_PageUp:
            case Qt.Key_PageDown:
            case Qt.Key_Tab:
            case Qt.Key_Backtab:
                // Not the view's: the keyboard stays in the name.
                event.accepted = true;
                break;
            case Qt.Key_F2:
                // As in Dolphin: the whole name, then the name alone again.
                if (input.selectedText.length === input.text.length) {
                    input.select(0, ed.stem);
                } else {
                    input.selectAll();
                }
                event.accepted = true;
                break;
            }
        }
        // Clicking elsewhere (a button) takes the keyboard away. (A field that
        // goes because its row now shows another item, or because the row's
        // new place made another field, is not the user leaving.)
        onActiveFocusChanged: {
            // (A row put away for reuse is hidden; that is not the user leaving.)
            if (!activeFocus && ed.visible && ed.Window.active && ed.fv.activeEditor === ed && ed.fv.renameUrl.toString() === ed.target.toString()) {
                ed.leave(false);
            }
        }
        Accessible.name: qsTr("Name")
        Accessible.role: Accessible.EditableText
    }
    property int stem: 0

    // Nothing the field doesn't use goes on to the view (Left at the start of
    // the name would move the selection).
    Keys.onPressed: event => event.accepted = true

    Component.onCompleted: {
        target = itemUrl;
        const info = actions.editableName(itemUrl, isDir);
        original = info.name;
        stem = info.stem;
        // Made again (the row was scrolled away and back, or the folder
        // changed under it): what was typed. Else the name, with its stem selected.
        const again = fv.renameDirty;
        input.text = again ? fv.renameText : info.name;
        ready = true;
        evaluate();
        input.forceActiveFocus();
        if (again) {
            input.cursorPosition = input.text.length;
        } else {
            input.select(0, stem);
        }
        fv.activeEditor = ed;
    }
    Component.onDestruction: {
        // The row went away under the edit (scrolled off): the edit ends as a click away does.
        if (fv.activeEditor === ed) {
            if (!finished) {
                leave(false);
            }
            fv.activeEditor = null;
        }
    }

    // Why the name can't be used, or what to look at, under the field.
    QQC2.Popup {
        parent: ed
        x: 0
        y: ed.height + Kirigami.Units.smallSpacing
        width: Math.max(ed.width, Kirigami.Units.gridUnit * 14)
        padding: Kirigami.Units.smallSpacing * 2
        focus: false
        modal: false
        closePolicy: QQC2.Popup.NoAutoClose
        visible: ed.message.length > 0 && !ed.finished
        background: Rectangle {
            radius: TelamonStyle.radiusSmall
            color: Kirigami.Theme.backgroundColor
            border.width: 1
            border.color: ed.refused ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.neutralTextColor
        }
        contentItem: Text {
            text: ed.message
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            color: Kirigami.Theme.textColor
            Accessible.role: Accessible.AlertMessage
            Accessible.name: text
        }
    }
}
