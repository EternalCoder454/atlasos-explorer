pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtMultimedia
import org.kde.kirigami as Kirigami
import Telamon.Ui

// Play controls for an audio or video file of this computer. The file is not
// opened, let alone played, until the play button is pressed: opening it
// means the media library parses it (a hostile file would be parsed by
// selecting it), so its length and picture size are known only from then on.
// The sound stops when this item goes away (the preview moves to another
// file, or closes). `output` is the VideoOutput the picture goes to (none for
// audio).
Item {
    id: root

    required property url source
    property VideoOutput output: null
    // For the details: the length and the picture size, once the file says.
    readonly property string durationText: player.duration > 0 ? PreviewLogic.durationText(player.duration) : ""
    readonly property string dimensionsText: {
        const r = player.metaData.value(MediaMetaData.Resolution);
        return r && r.width > 0 ? PreviewLogic.dimensionsText(r.width, r.height) : "";
    }
    // The player has the file open and is playing or paused (not stopped).
    readonly property bool started: player.playbackState !== MediaPlayer.StoppedState
    readonly property bool failed: player.error !== MediaPlayer.NoError

    implicitHeight: bar.implicitHeight + Kirigami.Units.smallSpacing * 2

    // Set by the first press of Play; a different file starts unarmed.
    property bool armed: false
    onSourceChanged: {
        player.stop();
        armed = false;
    }

    MediaPlayer {
        id: player
        source: root.armed ? root.source : ""
        videoOutput: root.output
        audioOutput: AudioOutput {}
    }

    Component.onDestruction: player.stop()

    RowLayout {
        id: bar
        anchors.fill: parent
        anchors.leftMargin: Kirigami.Units.smallSpacing
        anchors.rightMargin: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        ToolbarButton {
            symbol: player.playbackState === MediaPlayer.PlayingState ? Symbols.Pause : Symbols.PlayArrow
            text: player.playbackState === MediaPlayer.PlayingState ? qsTr("Pause") : qsTr("Play")
            enabled: !root.failed
            onClicked: {
                if (player.playbackState === MediaPlayer.PlayingState) {
                    player.pause();
                } else {
                    root.armed = true;
                    player.play();
                }
            }
        }
        TelamonSlider {
            Layout.fillWidth: true
            visible: !root.failed
            from: 0
            to: Math.max(1, player.duration)
            value: player.position
            enabled: player.seekable
            focusPolicy: Qt.NoFocus
            Accessible.name: qsTr("Position")
            onMoved: player.position = value
        }
        Text {
            Layout.fillWidth: root.failed
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: root.failed ? Kirigami.Theme.negativeTextColor : TelamonStyle.textMuted
            font.family: TelamonStyle.fontFamily
            font.pointSize: TelamonStyle.fontSizeCaption
            text: root.failed ? qsTr("This file can't be played.") : PreviewLogic.durationText(player.position) + " / " + (player.duration > 0 ? PreviewLogic.durationText(player.duration) : "–")
        }
    }
}
