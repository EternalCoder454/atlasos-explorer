pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import QtQuick.Shapes
import org.kde.kirigami as Kirigami
import Telamon.Ui

// The toolbar's ring for the operation queue: it fills while copies, moves and
// the like run, turns to a tick when they are done and to a warning when one
// didn't finish. A click opens the list, one row per operation, with its
// speed, the time left, Pause or Resume, Cancel, and Undo on the last one done.
ToolbarButton {
    id: button

    // The FileActions' OperationQueue.
    required property var queue
    readonly property bool busy: queue.active
    // 0..1, or -1 while it cannot be measured.
    readonly property real fraction: queue.progress
    readonly property real stroke: 2.5
    property double closedAt: 0

    visible: queue.anyListed
    text: qsTr("Operations")
    toolTipText: queue.summary
    focusable: true
    onClicked: {
        if (popover.visible) {
            popover.close();
        } else if (Date.now() - closedAt > 250) {
            popover.open();
        }
    }
    Accessible.description: queue.summary

    contentItem: Item {
        implicitWidth: Kirigami.Units.iconSizes.smallMedium
        implicitHeight: implicitWidth

        // A ring: its track, and the part done.
        Shape {
            id: ring
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            rotation: 0
            visible: button.busy
            ShapePath {
                fillColor: "transparent"
                strokeColor: TelamonStyle.alpha(Kirigami.Theme.textColor, 0.2)
                strokeWidth: button.stroke
                PathAngleArc {
                    centerX: ring.width / 2
                    centerY: ring.height / 2
                    radiusX: (ring.width - button.stroke) / 2
                    radiusY: radiusX
                    startAngle: 0
                    sweepAngle: 360
                }
            }
            ShapePath {
                fillColor: "transparent"
                strokeColor: button.queue.allPaused ? TelamonStyle.alpha(Kirigami.Theme.textColor, 0.5) : TelamonStyle.accent
                strokeWidth: button.stroke
                capStyle: ShapePath.RoundCap
                PathAngleArc {
                    centerX: ring.width / 2
                    centerY: ring.height / 2
                    radiusX: (ring.width - button.stroke) / 2
                    radiusY: radiusX
                    startAngle: -90
                    // Not measurable yet: a quarter turns round.
                    sweepAngle: button.fraction < 0 ? 90 : Math.max(8, 360 * button.fraction)
                }
            }
            RotationAnimator on rotation {
                running: ring.visible && button.fraction < 0 && !button.queue.allPaused && !TelamonStyle.reducedMotion
                from: 0
                to: 360
                duration: 1100
                loops: Animation.Infinite
            }
        }
        // Nothing running: the last result.
        Symbol {
            anchors.centerIn: parent
            visible: !button.busy
            icon: button.queue.hasFailed ? Symbols.Error : Symbols.Check
            size: Kirigami.Units.iconSizes.smallMedium
            color: button.queue.hasFailed ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
        }
    }

    TelamonPopover {
        id: popover
        target: button
        onClosed: button.closedAt = Date.now()

        ColumnLayout {
            id: column
            spacing: TelamonStyle.spacingLarge
            Layout.preferredWidth: Kirigami.Units.gridUnit * 28

            Text {
                text: qsTr("Operations")
                font.family: TelamonStyle.fontFamily
                font.pointSize: TelamonStyle.fontSizeBody
                font.weight: Font.DemiBold
                color: Kirigami.Theme.textColor
                textFormat: Text.PlainText
            }

            ListView {
                id: list
                Layout.fillWidth: true
                Layout.preferredWidth: Kirigami.Units.gridUnit * 28
                Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 22)
                clip: true
                spacing: TelamonStyle.spacingLarge
                model: button.queue
                boundsBehavior: Flickable.StopAtBounds

                delegate: ColumnLayout {
                    id: row
                    required property int opId
                    required property string label
                    required property string state
                    required property real progress
                    required property string detail
                    required property string error
                    required property bool canPause
                    required property bool canResume
                    required property bool canCancel
                    required property bool canRunNow
                    required property bool finished
                    required property bool undoable
                    width: list.width
                    spacing: 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: TelamonStyle.spacingSmall
                        Text {
                            Layout.fillWidth: true
                            text: row.label
                            elide: Text.ElideMiddle
                            font.family: TelamonStyle.fontFamily
                            font.pointSize: TelamonStyle.fontSizeBody
                            color: Kirigami.Theme.textColor
                            textFormat: Text.PlainText
                        }
                        TextButton {
                            visible: row.canPause
                            text: qsTr("Pause")
                            onClicked: button.queue.pause(row.opId)
                        }
                        TextButton {
                            visible: row.canResume
                            text: qsTr("Resume")
                            onClicked: button.queue.resume(row.opId)
                        }
                        TextButton {
                            visible: row.canRunNow
                            text: qsTr("Run Now")
                            onClicked: button.queue.runNow(row.opId)
                        }
                        TextButton {
                            visible: row.canCancel
                            text: qsTr("Cancel")
                            onClicked: button.queue.cancel(row.opId)
                        }
                        TextButton {
                            visible: row.undoable
                            text: qsTr("Undo")
                            onClicked: button.queue.undo()
                        }
                        TextButton {
                            visible: row.finished
                            text: qsTr("Dismiss")
                            onClicked: button.queue.dismiss(row.opId)
                        }
                    }
                    TelamonProgressBar {
                        Layout.fillWidth: true
                        visible: !row.finished
                        value: Math.max(0, row.progress)
                        indeterminate: row.progress < 0 && row.state === "running"
                        status: row.state === "paused" ? "paused" : "normal"
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: text.length > 0
                        text: row.state === "failed" && row.error.length > 0 ? row.error : row.detail
                        wrapMode: Text.Wrap
                        font.family: TelamonStyle.fontFamily
                        font.pointSize: TelamonStyle.fontSizeCaption
                        color: row.state === "failed" ? Kirigami.Theme.negativeTextColor : TelamonStyle.textMuted
                        textFormat: Text.PlainText
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Item {
                    Layout.fillWidth: true
                }
                TextButton {
                    text: qsTr("Clear Finished")
                    onClicked: {
                        button.queue.dismiss(0);
                        if (!button.queue.anyListed) {
                            popover.close();
                        }
                    }
                }
            }
        }
    }
}
