#include "TagLogic.h"

#include "PropsBridge.h"
#include "RustBridge.h"
#include "SearchController.h"

#include <KConfigGroup>
#include <KSharedConfig>

namespace
{
KConfigGroup tagGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Tags"));
}

// Names kept for the sidebar from the folders shown; far more than anyone has.
constexpr qsizetype MaxSeen = 300;
}

TagLogic *TagLogic::s_instance = nullptr;

TagLogic::TagLogic(QObject *parent)
    : QObject(parent)
{
    s_instance = this;
    m_used = tagGroup().readEntry("InUse", false);
    m_later.setSingleShot(true);
    m_later.setInterval(400);
    connect(&m_later, &QTimer::timeout, this, &TagLogic::refresh);
    // The index finished (re)reading: its answer may have changed.
    connect(SearchService::instance(), &SearchService::stateChanged, this, &TagLogic::schedule);
    // At start the index is asked only when tags were used before (it starts the service).
    QTimer::singleShot(0, this, &TagLogic::refresh);
}

TagLogic::~TagLogic()
{
    if (s_instance == this) {
        s_instance = nullptr;
    }
}

QString TagLogic::colourFor(const QString &name)
{
    static QHash<QString, QString> cache;
    const auto it = cache.constFind(name);
    if (it != cache.cend()) {
        return *it;
    }
    const QString hex = PropsBridge::colourOf(name);
    if (cache.size() < 2000) {
        cache.insert(name, hex);
    }
    return hex;
}

QStringList TagLogic::dotsFor(const QStringList &tags)
{
    QStringList out;
    for (const QString &t : tags) {
        const QString c = colourFor(t);
        if (!c.isEmpty() && !out.contains(c)) {
            out << c;
        }
    }
    return out;
}

void TagLogic::noteSeen(const QStringList &names)
{
    if (!s_instance || names.isEmpty()) {
        return;
    }
    bool added = false;
    for (const QString &n : names) {
        if (s_instance->m_seen.size() < MaxSeen && !s_instance->m_seen.contains(n, Qt::CaseInsensitive)) {
            s_instance->m_seen << n;
            added = true;
        }
    }
    if (added) {
        s_instance->rebuild();
    }
}

void TagLogic::noteChanged()
{
    if (s_instance && !s_instance->m_used) {
        s_instance->m_used = true;
        KConfigGroup g = tagGroup();
        g.writeEntry("InUse", true);
        g.sync();
    }
    if (s_instance) {
        s_instance->schedule();
    }
}

QVariantList TagLogic::colours() const
{
    QVariantList out;
    const QByteArray all = PropsBridge::bytesOf([](uint8_t *o, size_t c) { return telamon_tags_colours(o, c); });
    for (const QString &line : QString::fromUtf8(all).split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        const QStringList f = line.split(QLatin1Char('\t'));
        out << QVariantMap{{QStringLiteral("name"), f.value(0)}, {QStringLiteral("colour"), f.value(1)}};
    }
    return out;
}

void TagLogic::schedule()
{
    m_later.start();
}

void TagLogic::refresh()
{
    // Quiet while nobody used tags yet, so Files doesn't start the index for
    // nothing; the first search starts it and its state change comes here.
    SearchService::instance()->tags(this, m_used, [this](bool ok, const QList<QPair<QString, uint>> &list) {
        if (ok) {
            m_indexed = list;
            rebuild();
        }
    });
}

void TagLogic::rebuild()
{
    QByteArray seen = m_seen.join(QLatin1Char('\n')).toUtf8();
    QByteArray indexed;
    for (const auto &p : std::as_const(m_indexed)) {
        indexed += p.first.toUtf8() + '\t' + QByteArray::number(p.second) + '\n';
    }
    const QString text = QString::fromUtf8(PropsBridge::bytesOf(
        [&](uint8_t *o, size_t c) { return telamon_tags_in_use(PropsBridge::p(seen), PropsBridge::n(seen), PropsBridge::p(indexed), PropsBridge::n(indexed), o, c); }));
    QVariantList list;
    for (const QString &line : text.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        const QStringList f = line.split(QLatin1Char('\t'));
        const QString name = f.value(0);
        list << QVariantMap{{QStringLiteral("name"), name},
                            {QStringLiteral("text"), rustDisplayName(name.toUtf8())},
                            {QStringLiteral("colour"), colourFor(name)},
                            {QStringLiteral("count"), f.value(1).toUInt()}};
    }
    if (list != m_sidebar) {
        m_sidebar = list;
        Q_EMIT sidebarChanged();
    }
}

QVariantMap TagLogic::checkName(const QString &typed) const
{
    const QByteArray t = typed.toUtf8();
    uint32_t problem = 0;
    const QByteArray out = PropsBridge::bytesOf([&](uint8_t *o, size_t c) { return telamon_tags_new_name(PropsBridge::p(t), PropsBridge::n(t), o, c, &problem); });
    return {{QStringLiteral("ok"), problem == 0}, {QStringLiteral("name"), problem == 0 ? QString::fromUtf8(out) : QString()}, {QStringLiteral("text"), problem == 0 ? QString() : QString::fromUtf8(out)}};
}

QString TagLogic::shown(const QString &name) const
{
    return rustDisplayName(name.toUtf8());
}
