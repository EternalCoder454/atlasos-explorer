// Quick Look's and the preview pane's settings, and the zoom of the rows:
// the row height (Ctrl+scroll, Ctrl+plus, Ctrl+minus, Ctrl+0), and whether
// the preview pane (Alt+P) is shown. Both are kept in telamon-explorerrc,
// [View]. The limits and steps are the core's (atlas_explorer_core::zoom).
// The size of the icons is each folder's own (ViewMemory).
#pragma once

#include <QObject>
#include <QQmlEngine>

class PreviewLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // Pixels: the rows of the Details and Compact views.
    Q_PROPERTY(int rowHeight READ rowHeight NOTIFY rowHeightChanged)
    // What the rows' height is until the user changes it (two grid units), set by the window.
    Q_PROPERTY(int rowDefault READ rowDefault WRITE setRowDefault NOTIFY rowHeightChanged)
    Q_PROPERTY(bool paneShown READ paneShown WRITE setPaneShown NOTIFY paneShownChanged)

public:
    explicit PreviewLogic(QObject *parent = nullptr);

    int rowHeight() const;
    int rowDefault() const { return m_rowDefault; }
    void setRowDefault(int px);
    bool paneShown() const { return m_pane; }
    void setPaneShown(bool on);

    // Taller (steps > 0) or shorter rows, kept for the next start.
    Q_INVOKABLE void zoom(int steps);
    // Back to the default height.
    Q_INVOKABLE void resetZoom();
    // A wheel movement (angleDelta.y) with Ctrl held: the height changes by the steps it adds up to.
    Q_INVOKABLE void zoomByWheel(int delta);
    // The duration (milliseconds) or the dimensions as the details write them.
    Q_INVOKABLE QString durationText(qint64 ms) const;
    Q_INVOKABLE QString dimensionsText(int width, int height) const;

Q_SIGNALS:
    void rowHeightChanged();
    void paneShownChanged();

private:
    // 0: the default
    int m_row;
    int m_rowDefault = 36;
    bool m_pane;
    int m_wheelRest = 0;
};
