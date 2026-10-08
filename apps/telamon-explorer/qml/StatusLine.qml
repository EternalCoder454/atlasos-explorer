import QtQuick
import QtQuick.Layouts
import Telamon.Ui

// The slim line along the bottom of the window: how many items the folder
// has, how many of them are selected and how big (files only), how many are
// hidden, and the free space of the disk. Plain text, e.g.
// "12 items, 5 hidden, 3 selected (4.2 MB), 128 GB free".
StatusBar {
    id: bar

    // The tab's FolderModel.
    property var folder: null
    // {files, folders, bytes} of the selection (FolderModel.selectionStats).
    property var selection: ({
            "files": 0,
            "folders": 0,
            "bytes": 0
        })
    // Free bytes of the disk the folder is on; -1 when there is no such number.
    property real freeBytes: -1

    readonly property int selectedCount: selection.files + selection.folders
    readonly property string summary: {
        if (!folder || folder.errorText.length > 0) {
            return "";
        }
        const parts = [];
        const n = folder.count;
        parts.push(folder.loading && n === 0 ? qsTr("Loading…") : (n === 1 ? qsTr("1 item") : qsTr("%1 items").arg(n)));
        if (folder.hiddenCount > 0) {
            parts.push(qsTr("%1 hidden").arg(folder.hiddenCount));
        }
        if (selectedCount > 0) {
            // Folders add no size: only files are summed.
            parts.push(selection.files > 0 ? qsTr("%1 selected (%2)").arg(selectedCount).arg(LocationLogic.sizeText(selection.bytes)) : qsTr("%1 selected").arg(selectedCount));
        }
        if (freeBytes >= 0) {
            parts.push(qsTr("%1 free").arg(LocationLogic.sizeText(freeBytes)));
        }
        return parts.join(", ");
    }

    StatusBarItem {
        text: bar.summary
        Accessible.name: bar.summary
    }
    Item {
        Layout.fillWidth: true
    }
}
