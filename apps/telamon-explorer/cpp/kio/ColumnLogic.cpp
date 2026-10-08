#include "ColumnLogic.h"

#include <KConfigGroup>
#include <KSharedConfig>

namespace
{
KConfigGroup viewGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("View"));
}
}

ColumnLogic::ColumnLogic(QObject *parent)
    : QObject(parent)
{
    const KConfigGroup g = viewGroup();
    m_tags = g.readEntry("ColumnTags", false);
    m_dimensions = g.readEntry("ColumnDimensions", false);
    m_duration = g.readEntry("ColumnDuration", false);
    m_taken = g.readEntry("ColumnTaken", false);
}

void ColumnLogic::set(bool &field, bool on, const char *key)
{
    if (field == on) {
        return;
    }
    field = on;
    KConfigGroup g = viewGroup();
    g.writeEntry(key, on);
    g.sync();
    Q_EMIT changed();
}

void ColumnLogic::setTags(bool on)
{
    set(m_tags, on, "ColumnTags");
}

void ColumnLogic::setDimensions(bool on)
{
    set(m_dimensions, on, "ColumnDimensions");
}

void ColumnLogic::setDuration(bool on)
{
    set(m_duration, on, "ColumnDuration");
}

void ColumnLogic::setTaken(bool on)
{
    set(m_taken, on, "ColumnTaken");
}
