// Quick Look's and the preview pane's settings, and the zoom of the views:
// the icon size and the row height (Ctrl+scroll, Ctrl+plus, Ctrl+minus,
// Ctrl+0), and whether the preview pane (Alt+P) is shown. All three are kept
// in telamon-explorerrc, [View]. The limits and steps are the core's
// (atlas_explorer_core::zoom).
#pragma once

#include <QObject>
#include <QQmlEngine>

class PreviewLogic : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    // Pixels: the Icons view's icons, and the rows of the Details and Compact views.
    Q_PROPERTY(int iconSize READ iconSize NOTIFY iconSizeChanged)
    Q_PROPERTY(int rowHeight READ rowHeight NOTIFY rowHeightChanged)
    // What the rows' height is until the user changes it (two grid units), set by the window.
    Q_PROPERTY(int rowDefault READ rowDefault WRITE setRowDefault NOTIFY rowHeightChanged)
    Q_PROPERTY(bool paneShown READ paneShown WRITE setPaneShown NOTIFY paneShownChanged)

public:
    explicit PreviewLogic(QObject *parent = nullptr);

    int iconSize() const { return m_icon; }
    int rowHeight() const;
    int rowDefault() const { return m_rowDefault; }
    void setRowDefault(int px);
    bool paneShown() const { return m_pane; }
    void setPaneShown(bool on);

    // Bigger (steps > 0) or smaller icons, or rows, kept for the next start.
    Q_INVOKABLE void zoom(bool icons, int steps);
    // Back to the default size.
    Q_INVOKABLE void resetZoom(bool icons);
    // A wheel movement (angleDelta.y) with Ctrl held: the size changes by the steps it adds up to.
    Q_INVOKABLE void zoomByWheel(bool icons, int delta);
    // The duration (milliseconds) or the dimensions as the details write them.
    Q_INVOKABLE QString durationText(qint64 ms) const;
    Q_INVOKABLE QString dimensionsText(int width, int height) const;

Q_SIGNALS:
    void iconSizeChanged();
    void rowHeightChanged();
    void paneShownChanged();

private:
    int m_icon;
    // 0: the default
    int m_row;
    int m_rowDefault = 36;
    bool m_pane;
    int m_wheelRest = 0;
};
