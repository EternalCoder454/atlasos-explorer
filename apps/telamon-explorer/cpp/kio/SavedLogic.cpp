#include "SavedLogic.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QDir>

#include <functional>
#include <QUrl>

namespace
{
KConfigGroup savedGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("SavedSearches"));
}

// One field of a record, as the core reads it: every character that is not plain is %XX.
QByteArray field(const QVariant &v)
{
    return v.toString().toUtf8().toPercentEncoding();
}

// The record of a search, without its id, for the core.
QByteArray recordOf(const QString &name, const QVariantMap &s)
{
    const auto num = [&](const char *key) { return QByteArray::number(s.value(QLatin1String(key)).toInt()); };
    const auto flag = [&](const char *key) { return QByteArray(s.value(QLatin1String(key)).toBool() ? "1" : "0"); };
    QList<QByteArray> f;
    f << name.toUtf8().toPercentEncoding() << field(s.value(QStringLiteral("query"))) << num("scope") << field(s.value(QStringLiteral("folder"))) << num("kind")
      << num("modified") << num("size") << field(s.value(QStringLiteral("tag"))) << flag("pattern") << flag("contents");
    return f.join('\t');
}

QByteArray callText(const std::function<size_t(uint8_t *, size_t)> &fn)
{
    QByteArray buf(4096, 0);
    size_t n = fn(reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    if (n > size_t(buf.size())) {
        buf.resize(qsizetype(n));
        n = fn(reinterpret_cast<uint8_t *>(buf.data()), size_t(buf.size()));
    }
    buf.truncate(qsizetype(n));
    return buf;
}

QString decode(const QByteArray &f)
{
    return QString::fromUtf8(QByteArray::fromPercentEncoding(f));
}
}

SavedLogic::SavedLogic(QObject *parent)
    : QObject(parent)
{
    // Whatever the settings file holds, the core keeps what is fine.
    const QByteArray raw = savedGroup().readEntry("Items", QString()).toUtf8();
    reload(callText([&](uint8_t *out, size_t cap) { return telamon_saved_clean(reinterpret_cast<const uint8_t *>(raw.constData()), size_t(raw.size()), out, cap); }));
}

int SavedLogic::maxNameLength() const
{
    return int(telamon_saved_limit(1));
}

void SavedLogic::reload(const QByteArray &text)
{
    m_text = text;
    m_items.clear();
    for (const QByteArray &line : text.split('\n')) {
        const QList<QByteArray> f = line.split('\t');
        if (f.size() < 11) {
            continue;
        }
        const int id = f[0].toInt();
        QVariantMap m;
        m.insert(QStringLiteral("id"), id);
        m.insert(QStringLiteral("name"), decode(f[1]));
        m.insert(QStringLiteral("query"), decode(f[2]));
        m.insert(QStringLiteral("scope"), f[3].toInt());
        m.insert(QStringLiteral("folder"), decode(f[4]));
        m.insert(QStringLiteral("kind"), f[5].toInt());
        m.insert(QStringLiteral("modified"), f[6].toInt());
        m.insert(QStringLiteral("size"), f[7].toInt());
        m.insert(QStringLiteral("tag"), decode(f[8]));
        m.insert(QStringLiteral("pattern"), f[9].toInt() == 1);
        m.insert(QStringLiteral("contents"), f[10].toInt() == 1);
        const QByteArray label = m.value(QStringLiteral("folder")).toString().isEmpty() ? QByteArray() : rustSearchPathText(QUrl::fromEncoded(decode(f[4]).toUtf8()).toString(QUrl::FullyEncoded | QUrl::RemovePassword), QDir::homePath()).toUtf8();
        const QByteArray tip = callText([&](uint8_t *out, size_t cap) {
            return telamon_saved_describe(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), uint32_t(id),
                                          reinterpret_cast<const uint8_t *>(label.constData()), size_t(label.size()), out, cap);
        });
        m.insert(QStringLiteral("tip"), QString::fromUtf8(tip));
        m_items.append(m);
    }
}

void SavedLogic::keep(const QByteArray &text)
{
    KConfigGroup g = savedGroup();
    g.writeEntry("Items", QString::fromUtf8(text));
    g.sync();
    reload(text);
    Q_EMIT changed();
}

QString SavedLogic::suggestName(const QVariantMap &search) const
{
    const QByteArray rec = recordOf(QString(), search);
    return QString::fromUtf8(callText([&](uint8_t *out, size_t cap) { return telamon_saved_default_name(reinterpret_cast<const uint8_t *>(rec.constData()), size_t(rec.size()), out, cap); }));
}

int SavedLogic::save(const QString &name, const QVariantMap &search)
{
    const QByteArray rec = recordOf(name, search);
    uint32_t status = 1;
    const QByteArray text = callText([&](uint8_t *out, size_t cap) {
        return telamon_saved_add(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), reinterpret_cast<const uint8_t *>(rec.constData()), size_t(rec.size()), out, cap,
                                 &status);
    });
    if (status == 0) {
        keep(text);
    }
    return int(status);
}

int SavedLogic::rename(int id, const QString &name)
{
    const QByteArray n = name.toUtf8();
    uint32_t status = 1;
    const QByteArray text = callText([&](uint8_t *out, size_t cap) {
        return telamon_saved_rename(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), uint32_t(id), reinterpret_cast<const uint8_t *>(n.constData()), size_t(n.size()), out,
                                    cap, &status);
    });
    if (status == 0) {
        keep(text);
    }
    return int(status);
}

void SavedLogic::remove(int id)
{
    const QByteArray text = callText([&](uint8_t *out, size_t cap) {
        return telamon_saved_remove(reinterpret_cast<const uint8_t *>(m_text.constData()), size_t(m_text.size()), uint32_t(id), out, cap);
    });
    if (text != m_text) {
        keep(text);
    }
}

QVariantMap SavedLogic::get(int id) const
{
    for (const QVariant &v : m_items) {
        const QVariantMap m = v.toMap();
        if (m.value(QStringLiteral("id")).toInt() == id) {
            return m;
        }
    }
    return {};
}

QString SavedLogic::nameOf(int id) const
{
    return get(id).value(QStringLiteral("name")).toString();
}
