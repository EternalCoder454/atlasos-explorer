pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The preview pane (Alt+P): the file selected in the view, as a picture, its
// text or a player, with its name, kind, size, dates and, where known, its
// picture size or length. It never takes the keyboard from the view.
Item {
    id: pane

    // The FolderView of the tab shown (null for none).
    property var view: null
    // Another preview (Quick Look) is on top: no player runs here meanwhile.
    property bool covered: false

    // What the pane shows: nothing, one file, or several.
    property var info: ({})
    property int selectedCount: 0
    property string manyText: ""

    function refresh() {
        const v = pane.view;
        if (!v) {
            selectedCount = 0;
            info = ({});
            return;
        }
        const rows = v.selectedRows();
        selectedCount = rows.length;
        if (rows.length === 1) {
            info = v.folder.detailsAt(rows[0]);
            manyText = "";
        } else {
            info = ({});
            if (rows.length > 1) {
                const s = v.folder.selectionStats(rows);
                manyText = s.files > 0 ? qsTr("%1 in the files").arg(LocationLogic.sizeText(s.bytes)) : "";
            }
        }
    }
    onViewChanged: refresh()
    Component.onCompleted: refresh()

    Connections {
        target: pane.view
        function onSelRevisionChanged() {
            pane.refresh();
        }
    }
    Connections {
        target: pane.view ? pane.view.folder : null
        function onModelReset() {
            pane.refresh();
        }
        function onLayoutChanged() {
            pane.refresh();
        }
        function onDataChanged() {
            pane.refresh();
        }
    }

    // Only a file that has something to show has a preview here.
    readonly property bool single: selectedCount === 1 && info.url !== undefined

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.largeSpacing
        visible: pane.single

        PreviewBody {
            id: body
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: Kirigami.Units.gridUnit * 8
            info: pane.info
            playerActive: !pane.covered
            thumbSide: 512
            iconSide: Kirigami.Units.iconSizes.huge
        }

        // Name, kind, size, dates and, where known, the picture's size or the
        // length: a muted label and its value, as plain text.
        Column {
            id: details
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            readonly property var rows: {
                const all = [
                    {
                        "label": qsTr("Name"),
                        "value": pane.info.name ?? ""
                    },
                    {
                        "label": qsTr("Kind"),
                        "value": body.loader.kindText || (pane.info.typeText ?? "")
                    },
                    {
                        "label": qsTr("Size"),
                        "value": pane.info.sizeText ?? ""
                    },
                    {
                        "label": qsTr("Dimensions"),
                        "value": body.loader.dimensionsText || body.mediaDimensions
                    },
                    {
                        "label": qsTr("Duration"),
                        "value": body.mediaDuration
                    },
                    {
                        "label": qsTr("Modified"),
                        "value": pane.info.modifiedText ?? ""
                    },
                    {
                        "label": qsTr("Created"),
                        "value": pane.info.createdText ?? ""
                    },
                    {
                        "label": qsTr("Where"),
                        "value": pane.info.pathText ?? ""
                    }
                ];
                return all.filter(r => r.value && r.value.length > 0);
            }
            Repeater {
                model: details.rows
                delegate: RowLayout {
                    id: entry
                    required property var modelData
                    width: details.width
                    spacing: Kirigami.Units.largeSpacing
                    Text {
                        Layout.alignment: Qt.AlignTop
                        Layout.preferredWidth: Kirigami.Units.gridUnit * 5
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        color: TelamonStyle.textMuted
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeCaption
                        text: entry.modelData.label
                    }
                    Text {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        textFormat: Text.PlainText
                        wrapMode: Text.WrapAnywhere
                        maximumLineCount: 4
                        elide: Text.ElideRight
                        color: Kirigami.Theme.textColor
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeCaption
                        text: entry.modelData.value
                    }
                }
            }
        }
    }

    TelamonEmptyState {
        anchors.centerIn: parent
        width: parent.width - Kirigami.Units.gridUnit * 2
        visible: !pane.single
        symbol: Symbols.Visibility
        title: pane.selectedCount > 1 ? qsTr("%1 Items Selected").arg(pane.selectedCount) : qsTr("No Preview")
        text: pane.selectedCount > 1 ? pane.manyText : qsTr("Select a file to see it here.")
    }
}
