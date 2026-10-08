#include "ViewMemory.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QCoreApplication>

namespace
{
constexpr uint32_t Icons = 0;

KConfigGroup viewGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("View"));
}

KConfigGroup listGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("FolderViews"));
}

QString modeName(uint32_t code)
{
    QByteArray buf(16, 0);
    const size_t n = telamon_views_mode_name(code, reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    return n > 0 && n <= size_t(buf.size()) ? QString::fromLatin1(buf.constData(), qsizetype(n)) : QStringLiteral("details");
}

int modeCode(const QString &name)
{
    const QByteArray n = name.toUtf8();
    return int(telamon_views_mode_code(reinterpret_cast<const uint8_t *>(n.constData()), size_t(n.size())));
}

QVariantMap prefsMap(const TelamonViewPrefs &p, bool remembered)
{
    return {{QStringLiteral("mode"), modeName(p.mode)},
            {QStringLiteral("sort"), int(p.sort)},
            {QStringLiteral("descending"), p.descending},
            {QStringLiteral("icon"), int(p.icon)},
            {QStringLiteral("group"), int(p.group)},
            {QStringLiteral("remembered"), remembered}};
}
}

ViewMemory::ViewMemory(QObject *parent)
    : QObject(parent)
{
    const KConfigGroup v = viewGroup();
    // A settings file can hold anything: the core brings it into the limits.
    m_same = v.readEntry("SameViewForAll", false);
    m_mode = modeName(uint32_t(qMax(0, modeCode(v.readEntry("Mode", QStringLiteral("details"))))));
    m_sort = v.readEntry("SortColumn", 0);
    // The core's columns 0 to 5, and the Trash's 7 and 8 (6 is search relevance, never kept).
    if (m_sort < 0 || m_sort > 8 || m_sort == 6) {
        m_sort = 0;
    }
    m_desc = v.readEntry("SortDescending", false);
    m_icon = telamon_zoom_clamp(Icons, v.readEntry("IconSize", telamon_zoom_default(Icons, 0)));
    m_group = qBound(0, v.readEntry("GroupBy", 0), 3);

    // Lines that don't parse are dropped by the core on the first change.
    m_text = listGroup().readEntry("List", QStringList()).join(QLatin1Char('\n')).toUtf8();

    m_timer.setSingleShot(true);
    m_timer.setInterval(400);
    connect(&m_timer, &QTimer::timeout, this, &ViewMemory::flush);
    connect(qApp, &QCoreApplication::aboutToQuit, this, &ViewMemory::flush);
}

ViewMemory::~ViewMemory()
{
    flush();
}

QByteArray ViewMemory::key(const QUrl &folder) const
{
    if (!folder.isValid() || folder.isEmpty()) {
        return {};
    }
    // Never the password, a query or a fragment: the file holds locations.
    const QByteArray k = folder.adjusted(QUrl::RemovePassword | QUrl::RemoveQuery | QUrl::RemoveFragment | QUrl::StripTrailingSlash).toEncoded();
    return telamon_views_valid_key(reinterpret_cast<const uint8_t *>(k.constData()), size_t(k.size())) ? k : QByteArray();
}

QVariantMap ViewMemory::shared() const
{
    return {{QStringLiteral("mode"), m_mode},
            {QStringLiteral("sort"), m_sort},
            {QStringLiteral("descending"), m_desc},
            {QStringLiteral("icon"), m_icon},
            {QStringLiteral("group"), m_group},
            {QStringLiteral("remembered"), false}};
}

void ViewMemory::setSameForAll(bool on)
{
    if (on == m_same) {
        return;
    }
    m_same = on;
    KConfigGroup g = viewGroup();
    g.writeEntry("SameViewForAll", on);
    g.sync();
    Q_EMIT sameForAllChanged();
}

QVariantMap ViewMemory::prefsFor(const QUrl &folder) const
{
    if (m_same) {
        return shared();
    }
    const QByteArray k = key(folder);
    TelamonViewPrefs p{};
    if (!k.isEmpty()
        && telamon_views_get(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), reinterpret_cast<const uint8_t *>(k.constData()),
                             size_t(k.size()), &p)) {
        return prefsMap(p, true);
    }
    return shared();
}

bool ViewMemory::hasEntry(const QUrl &folder) const
{
    const QByteArray k = key(folder);
    TelamonViewPrefs p{};
    return !k.isEmpty()
        && telamon_views_get(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), reinterpret_cast<const uint8_t *>(k.constData()),
                             size_t(k.size()), &p);
}

int ViewMemory::entryCount() const
{
    return m_text.isEmpty() ? 0 : int(m_text.count('\n')) + (m_text.endsWith('\n') ? 0 : 1);
}

void ViewMemory::remember(const QUrl &folder, const QString &mode, int sort, bool descending, int icon, int group)
{
    const int code = modeCode(mode);
    TelamonViewPrefs p{uint32_t(qMax(0, code)), uint32_t(qMax(0, sort)), descending, int32_t(icon), uint32_t(qMax(0, group))};
    if (m_same) {
        // The core's limits for what is kept, the same as for a folder's.
        m_mode = modeName(p.mode);
        m_sort = p.sort > 5 ? 0 : int(p.sort);
        m_desc = descending;
        m_icon = telamon_zoom_clamp(Icons, icon);
        m_group = qBound(0, group, 3);
        KConfigGroup g = viewGroup();
        g.writeEntry("Mode", m_mode);
        g.writeEntry("SortColumn", m_sort);
        g.writeEntry("SortDescending", m_desc);
        g.writeEntry("IconSize", m_icon);
        g.writeEntry("GroupBy", m_group);
        touch();
        return;
    }
    const QByteArray k = key(folder);
    if (k.isEmpty()) {
        return;
    }
    QByteArray out(m_text.size() + 512, 0);
    auto call = [&] {
        return telamon_views_set(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), reinterpret_cast<const uint8_t *>(k.constData()),
                                 size_t(k.size()), &p, reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
    };
    size_t n = call();
    if (n > size_t(out.size())) {
        out.resize(qsizetype(n));
        n = call();
    }
    out.truncate(qsizetype(n));
    m_text = out;
    touch();
}

void ViewMemory::forget(const QUrl &folder)
{
    const QByteArray k = key(folder);
    if (k.isEmpty()) {
        return;
    }
    QByteArray out(m_text.size() + 16, 0);
    const size_t n = telamon_views_forget(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()),
                                          reinterpret_cast<const uint8_t *>(k.constData()), size_t(k.size()), reinterpret_cast<uint8_t *>(out.data()),
                                          size_t(out.size()));
    if (n <= size_t(out.size())) {
        out.truncate(qsizetype(n));
        m_text = out;
        touch();
    }
}

// The settings are written shortly after the last change (a Ctrl+scroll is many).
void ViewMemory::touch()
{
    ++m_revision;
    Q_EMIT revisionChanged();
    m_dirty = true;
    m_timer.start();
}

void ViewMemory::flush()
{
    m_timer.stop();
    if (!m_dirty) {
        return;
    }
    m_dirty = false;
    KConfigGroup l = listGroup();
    const QString text = QString::fromUtf8(m_text);
    if (text.isEmpty()) {
        l.deleteEntry("List");
    } else {
        l.writeEntry("List", text.split(QLatin1Char('\n'), Qt::SkipEmptyParts));
    }
    l.sync();
    viewGroup().sync();
}

int ViewMemory::iconStep(int current, int steps) const
{
    return telamon_zoom_step(Icons, current, steps);
}

int ViewMemory::iconDefault() const
{
    return m_icon;
}

int ViewMemory::iconWheelSteps(int delta)
{
    int rest = 0;
    const int steps = telamon_zoom_wheel(m_wheelRest, delta, &rest);
    m_wheelRest = rest;
    return steps;
}
