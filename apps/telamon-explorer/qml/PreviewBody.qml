pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import QtMultimedia
import org.kde.kirigami as Kirigami
import Telamon.Ui

// What a file looks like in Quick Look and in the preview pane: a picture
// from KIO's thumbnailers, or its text (plain, never markup), or a player for
// audio and video, or its icon with a line saying why there is no preview.
// `info` is FolderModel.detailsAt(row); the file is read by PreviewLoader, not
// here. Nothing plays by itself.
Item {
    id: body

    property var info: ({})
    // False takes the player away (another preview is on top of this one).
    property bool playerActive: true
    // Pixels of the picture asked of the thumbnailers.
    property int thumbSide: 1024
    property int iconSide: Kirigami.Units.iconSizes.enormous
    readonly property alias loader: loader
    // From the player, once the file says.
    readonly property string mediaDuration: media.item ? media.item.durationText : ""
    readonly property string mediaDimensions: media.item ? media.item.dimensionsText : ""

    // The file that is shown: another size or date is another file.
    readonly property string key: info.url === undefined ? "" : info.url.toString() + "|" + info.size + "|" + info.mtime
    onKeyChanged: reload()
    Component.onCompleted: reload()

    function reload() {
        if (key.length === 0) {
            loader.clear();
        } else {
            loader.load(info.localPath, info.isDir, info.name, info.typeText);
        }
    }

    PreviewLoader {
        id: loader
    }

    readonly property bool isText: loader.category === PreviewLoader.Text
    readonly property bool isMedia: loader.playUrl.toString().length > 0 && !loader.busy
    readonly property bool pictureReady: thumb.status === Image.Ready && thumb.implicitWidth > 1

    // Text
    Item {
        anchors.fill: parent
        visible: body.isText && !loader.busy
        Flickable {
            id: flick
            anchors.fill: parent
            anchors.bottomMargin: note.visible ? note.height + Kirigami.Units.smallSpacing : 0
            clip: true
            contentWidth: width
            contentHeight: textView.height
            boundsBehavior: Flickable.StopAtBounds
            QQC2.ScrollBar.vertical: TelamonScrollBar {}
            // Home and End are the window's; the text scrolls with the wheel and the bar.
            TextEdit {
                id: textView
                width: flick.width - Kirigami.Units.largeSpacing
                readOnly: true
                selectByMouse: true
                activeFocusOnPress: false
                activeFocusOnTab: false
                // Never HTML or Markdown: the text is shown as the characters it is.
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.WrapAtWordBoundaryOrAnywhere
                text: body.isText ? loader.text : ""
                font.family: TelamonStyle.monoFamily
                font.pointSize: TelamonStyle.fontSizeBody
                color: Kirigami.Theme.textColor
                selectionColor: TelamonStyle.accent
                selectedTextColor: TelamonStyle.accentText
                Accessible.role: Accessible.StaticText
                Accessible.name: qsTr("Text of the file")
            }
        }
        Text {
            id: note
            visible: loader.note.length > 0
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            textFormat: Text.PlainText
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
            color: TelamonStyle.textMuted
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            text: loader.note
        }
    }

    // A picture, or the picture of a video, and its player
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: !body.isText
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            Image {
                id: thumb
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                source: loader.thumbnailSource
                sourceSize: Qt.size(body.thumbSide, body.thumbSide)
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                cache: false
                // A file with no thumbnail comes back 1x1 and the icon stays.
                visible: body.pictureReady
                Accessible.role: Accessible.Graphic
                Accessible.name: body.info.name ?? ""
            }
            // The picture of a video while it plays or is paused.
            VideoOutput {
                id: surface
                anchors.fill: parent
                anchors.margins: Kirigami.Units.smallSpacing
                fillMode: VideoOutput.PreserveAspectFit
                visible: loader.category === PreviewLoader.Video && media.item !== null && media.item.started
            }
            // No picture: the file's icon and why
            ColumnLayout {
                anchors.centerIn: parent
                width: Math.min(parent.width - Kirigami.Units.gridUnit, Kirigami.Units.gridUnit * 24)
                visible: !body.pictureReady && !loader.busy && !surface.visible && thumb.status !== Image.Loading
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: body.iconSide
                    Layout.preferredHeight: body.iconSide
                    source: body.info.iconName ?? "application-octet-stream"
                }
                Text {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    horizontalAlignment: Text.AlignHCenter
                    color: TelamonStyle.textMuted
                    font.family: TelamonStyle.fontFamily
                    font.pointSize: TelamonStyle.fontSizeBody
                    text: {
                        if (loader.note.length > 0) {
                            return loader.note;
                        }
                        if (loader.category === PreviewLoader.Folder) {
                            return "";
                        }
                        return qsTr("No preview is available for this file.");
                    }
                }
            }
            TelamonSpinner {
                anchors.centerIn: parent
                running: loader.busy || thumb.status === Image.Loading
            }
        }
        Loader {
            id: media
            Layout.fillWidth: true
            active: body.isMedia && body.playerActive
            visible: active
            sourceComponent: MediaControls {
                source: loader.playUrl
                output: surface
            }
        }
    }
}
