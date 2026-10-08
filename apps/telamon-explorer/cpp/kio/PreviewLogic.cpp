#include "PreviewLogic.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

namespace
{
constexpr uint32_t Icons = 0, Rows = 1;

KConfigGroup viewGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("View"));
}
}

PreviewLogic::PreviewLogic(QObject *parent)
    : QObject(parent)
{
    const KConfigGroup g = viewGroup();
    // A settings file can hold anything: the core brings it into the limits.
    m_icon = telamon_zoom_clamp(Icons, g.readEntry("IconSize", telamon_zoom_default(Icons, 0)));
    m_row = qMax(0, g.readEntry("RowHeight", 0));
    if (m_row > 0) {
        m_row = telamon_zoom_clamp(Rows, m_row);
    }
    m_pane = g.readEntry("PreviewPane", false);
}

int PreviewLogic::rowHeight() const
{
    return m_row > 0 ? m_row : telamon_zoom_default(Rows, m_rowDefault);
}

void PreviewLogic::setRowDefault(int px)
{
    if (px != m_rowDefault && px > 0) {
        const int before = rowHeight();
        m_rowDefault = px;
        if (rowHeight() != before) {
            Q_EMIT rowHeightChanged();
        }
    }
}

void PreviewLogic::setPaneShown(bool on)
{
    if (on == m_pane) {
        return;
    }
    m_pane = on;
    KConfigGroup g = viewGroup();
    g.writeEntry("PreviewPane", on);
    g.sync();
    Q_EMIT paneShownChanged();
}

void PreviewLogic::zoom(bool icons, int steps)
{
    KConfigGroup g = viewGroup();
    if (icons) {
        const int now = telamon_zoom_step(Icons, m_icon, steps);
        if (now != m_icon) {
            m_icon = now;
            g.writeEntry("IconSize", now);
            g.sync();
            Q_EMIT iconSizeChanged();
        }
    } else {
        const int now = telamon_zoom_step(Rows, rowHeight(), steps);
        if (now != rowHeight()) {
            m_row = now;
            g.writeEntry("RowHeight", now);
            g.sync();
            Q_EMIT rowHeightChanged();
        }
    }
}

void PreviewLogic::resetZoom(bool icons)
{
    KConfigGroup g = viewGroup();
    if (icons) {
        const int def = telamon_zoom_default(Icons, 0);
        g.deleteEntry("IconSize");
        if (m_icon != def) {
            m_icon = def;
            Q_EMIT iconSizeChanged();
        }
    } else {
        const int before = rowHeight();
        m_row = 0;
        g.deleteEntry("RowHeight");
        if (rowHeight() != before) {
            Q_EMIT rowHeightChanged();
        }
    }
    g.sync();
}

void PreviewLogic::zoomByWheel(bool icons, int delta)
{
    int rest = 0;
    const int steps = telamon_zoom_wheel(m_wheelRest, delta, &rest);
    m_wheelRest = rest;
    if (steps != 0) {
        zoom(icons, steps);
    }
}

QString PreviewLogic::durationText(qint64 ms) const
{
    return ms < 0 ? QString() : rustPreviewText(0, quint64(ms));
}

QString PreviewLogic::dimensionsText(int width, int height) const
{
    if (width <= 0 || height <= 0) {
        return QString();
    }
    return rustPreviewText(1, (quint64(width) << 32) | quint64(height));
}
