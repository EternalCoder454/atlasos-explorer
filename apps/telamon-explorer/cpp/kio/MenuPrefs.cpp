#include "MenuPrefs.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KDesktopFile>
#include <KPluginMetaData>
#include <KSharedConfig>

#include <QDir>
#include <QPointer>
#include <QStandardPaths>
#include <QThreadPool>

namespace
{
KConfigGroup menuGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Menu"));
}

QByteArray cleanedText()
{
    const QByteArray raw = menuGroup().readEntry("Hidden", QString()).toUtf8();
    return rustText([&](uint8_t *out, size_t cap) { return telamon_menuprefs_clean(rustPtr(raw), size_t(raw.size()), out, cap); });
}

// Plugins Files leaves out of the menu for its own reasons (see FileActions::itemMenu).
const QStringList &alwaysExcluded()
{
    static const QStringList l{QStringLiteral("compressfileitemaction"), QStringLiteral("extractfileitemaction"), QStringLiteral("kactivitymanagerd_fileitem_linking_plugin")};
    return l;
}

// The service menus found on this computer, as KIO's file-item actions read
// them: *.desktop files in kio/servicemenus (and the older kservices folders),
// each with its `Actions=`; then the plugins. Runs on a worker.
QVariantList findServices()
{
    QVariantList out;
    QSet<QString> seen;
    QStringList dirs;
    for (const QString &sub : {QStringLiteral("kio/servicemenus"), QStringLiteral("kservices6/ServiceMenus"), QStringLiteral("kservices5/ServiceMenus")}) {
        dirs += QStandardPaths::locateAll(QStandardPaths::GenericDataLocation, sub, QStandardPaths::LocateDirectory);
    }
    int files = 0;
    for (const QString &dir : std::as_const(dirs)) {
        const QStringList names = QDir(dir).entryList({QStringLiteral("*.desktop")}, QDir::Files | QDir::Readable, QDir::Name);
        for (const QString &fn : names) {
            if (++files > 300) {
                break;
            }
            KDesktopFile df(QDir(dir).filePath(fn));
            if (df.noDisplay() || df.desktopGroup().readEntry("Hidden", false)) {
                continue;
            }
            const QString group = df.readName();
            const QStringList actions = df.desktopGroup().readXdgListEntry(QStringLiteral("Actions"));
            for (const QString &a : actions.mid(0, 100)) {
                const QString key = QStringLiteral("svc:") + a;
                if (a.isEmpty() || seen.contains(key)) {
                    continue;
                }
                seen.insert(key);
                const KConfigGroup ag = df.actionGroup(a);
                QString text = ag.readEntry("Name", a);
                text = rustDisplayName(text.toUtf8());
                out.append(QVariantMap{{QStringLiteral("key"), key},
                                       {QStringLiteral("text"), text},
                                       {QStringLiteral("detail"), group.isEmpty() ? fn : rustDisplayName(group.toUtf8())},
                                       {QStringLiteral("kind"), QStringLiteral("service")}});
            }
        }
    }
    const QList<KPluginMetaData> plugins = KPluginMetaData::findPlugins(QStringLiteral("kf6/kfileitemaction"));
    for (const KPluginMetaData &m : plugins) {
        if (alwaysExcluded().contains(m.pluginId())) {
            continue;
        }
        const QString key = QStringLiteral("plugin:") + m.pluginId();
        if (seen.contains(key)) {
            continue;
        }
        seen.insert(key);
        out.append(QVariantMap{{QStringLiteral("key"), key},
                               {QStringLiteral("text"), rustDisplayName(m.name().toUtf8())},
                               {QStringLiteral("detail"), rustDisplayName(m.description().toUtf8())},
                               {QStringLiteral("kind"), QStringLiteral("plugin")}});
    }
    return out;
}
}

MenuPrefs::MenuPrefs(QObject *parent)
    : QObject(parent)
{
    m_text = cleanedText();
    m_builtin = QString::fromUtf8(rustText([](uint8_t *out, size_t cap) { return telamon_menuprefs_builtin(out, cap); }, 1024)).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
}

QByteArray MenuPrefs::hiddenText()
{
    return cleanedText();
}

QStringList MenuPrefs::excludedServices()
{
    const QByteArray t = cleanedText();
    QStringList l = QString::fromUtf8(rustText([&](uint8_t *out, size_t cap) { return telamon_menuprefs_excluded(rustPtr(t), size_t(t.size()), out, cap); })).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    l += alwaysExcluded();
    return l;
}

bool MenuPrefs::isHidden(const QString &key) const
{
    const QByteArray k = key.toUtf8();
    return m_text.split('\n').contains(k);
}

void MenuPrefs::setHidden(const QString &key, bool hidden)
{
    const QByteArray k = key.toUtf8();
    const QByteArray text = rustText([&](uint8_t *out, size_t cap) {
        return telamon_menuprefs_set(rustPtr(m_text), size_t(m_text.size()), rustPtr(k), size_t(k.size()), hidden, out, cap);
    });
    if (text == m_text) {
        return;
    }
    KConfigGroup g = menuGroup();
    if (text.isEmpty()) {
        g.deleteEntry("Hidden");
    } else {
        g.writeEntry("Hidden", QString::fromUtf8(text));
    }
    g.sync();
    m_text = text;
    ++m_revision;
    Q_EMIT changed();
}

void MenuPrefs::scan()
{
    const int serial = ++m_scanSerial;
    m_scanning = true;
    Q_EMIT servicesChanged();
    QPointer<MenuPrefs> self(this);
    QThreadPool::globalInstance()->start([self, serial] {
        const QVariantList found = findServices();
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, found, serial] {
            if (self && serial == self->m_scanSerial) {
                self->m_services = found;
                self->m_scanning = false;
                Q_EMIT self->servicesChanged();
            }
        });
    });
}
