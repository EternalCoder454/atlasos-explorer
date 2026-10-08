#include "SettingsLogic.h"

#include "RustBridge.h"
#include "SearchController.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QLocale>
#include <QSaveFile>
#include <QStandardPaths>

namespace
{
KConfigGroup cfgGroup(const char *name)
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QString::fromLatin1(name));
}

constexpr qint64 MaxIndexrc = 64 * 1024;
}

SettingsLogic::SettingsLogic(QObject *parent)
    : QObject(parent)
{
    m_startPage = cfgGroup("General").readEntry("StartPage", 0) == 1 ? 1 : 0;
    m_usePattern = cfgGroup("Search").readEntry("UsePattern", false);
    m_git = cfgGroup("View").readEntry("GitBadges", false);
    readFolders();
    connect(SearchService::instance(), &SearchService::stateChanged, this, &SettingsLogic::indexChanged);
}

void SettingsLogic::setStartPage(int page)
{
    page = page == 1 ? 1 : 0;
    if (page == m_startPage) {
        return;
    }
    m_startPage = page;
    KConfigGroup g = cfgGroup("General");
    g.writeEntry("StartPage", page);
    g.sync();
    Q_EMIT startPageChanged();
}

void SettingsLogic::setUsePattern(bool on)
{
    if (on == m_usePattern) {
        return;
    }
    m_usePattern = on;
    KConfigGroup g = cfgGroup("Search");
    g.writeEntry("UsePattern", on);
    g.sync();
    Q_EMIT usePatternChanged();
}

void SettingsLogic::setGitBadges(bool on)
{
    if (on == m_git) {
        return;
    }
    m_git = on;
    KConfigGroup g = cfgGroup("View");
    g.writeEntry("GitBadges", on);
    g.sync();
    Q_EMIT gitBadgesChanged();
}

QString SettingsLogic::indexrcPath() const
{
    return QStandardPaths::writableLocation(QStandardPaths::ConfigLocation) + QStringLiteral("/telamon-explorer/indexrc");
}

QByteArray SettingsLogic::readIndexrc() const
{
    QFile f(indexrcPath());
    if (!f.open(QIODevice::ReadOnly)) {
        return {};
    }
    // Bigger than the service reads: left alone, as if there were none.
    if (f.size() > MaxIndexrc) {
        return {};
    }
    return f.read(MaxIndexrc);
}

void SettingsLogic::readFolders()
{
    const QByteArray text = readIndexrc();
    const QByteArray home = QDir::homePath().toUtf8();
    const QString roots = QString::fromUtf8(rustText([&](uint8_t *out, size_t cap) { return telamon_indexrc_roots(rustPtr(text), size_t(text.size()), rustPtr(home), size_t(home.size()), out, cap); }));
    m_folders = roots.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
}

QString SettingsLogic::indexStatus() const
{
    if (indexOff()) {
        return tr("The file index is off: no folder is indexed.");
    }
    SearchService *svc = SearchService::instance();
    switch (svc->state()) {
    case SearchService::Ready:
        return tr("Ready: %1 items.").arg(QLocale().toString(qulonglong(svc->entries())));
    case SearchService::Updating:
        return tr("Updating: %1 items so far.").arg(QLocale().toString(qulonglong(svc->entries())));
    case SearchService::Problem:
        return svc->errorText().isEmpty() ? tr("The index has a problem.") : tr("The index has a problem: %1").arg(svc->errorText());
    case SearchService::Unavailable:
        return tr("The index service can't be reached.");
    case SearchService::Off:
        return tr("The file index is off.");
    default:
        return tr("The index starts when you first search.");
    }
}

void SettingsLogic::refreshIndex()
{
    readFolders();
    Q_EMIT indexChanged();
    // Opening Settings must not start the service (it scans the home folder when it starts): ask only if it runs.
    if (SearchService::instance()->running()) {
        SearchService::instance()->refreshStatus(this, [this] { Q_EMIT indexChanged(); });
    }
}

void SettingsLogic::rebuildIndex()
{
    if (indexOff()) {
        return;
    }
    SearchService::instance()->rebuild();
}

QString SettingsLogic::saveFolders(const QStringList &folders)
{
    // A file the service can't read as it is (too big) is not written over: its other lines would be lost.
    if (QFileInfo(indexrcPath()).size() > MaxIndexrc) {
        return tr("The settings file of the index is too big to change here.");
    }
    const QByteArray text = readIndexrc();
    const QByteArray roots = folders.join(QLatin1Char('\n')).toUtf8();
    uint32_t status = 1;
    const QByteArray out = rustText([&](uint8_t *o, size_t cap) { return telamon_indexrc_set_roots(rustPtr(text), size_t(text.size()), rustPtr(roots), size_t(roots.size()), o, cap, &status); }, 4096);
    if (status != 0) {
        return QString::fromUtf8(out);
    }
    const QString path = indexrcPath();
    QDir().mkpath(QFileInfo(path).absolutePath());
    QSaveFile f(path);
    if (!f.open(QIODevice::WriteOnly) || f.write(out) != out.size() || !f.commit()) {
        return tr("The settings of the index could not be saved.");
    }
    m_folders = folders;
    Q_EMIT indexChanged();
    // The service reads indexrc when it starts: it ends now and starts at the next search.
    SearchService::instance()->reload();
    return {};
}

QString SettingsLogic::addIndexFolder(const QString &path)
{
    const QFileInfo fi(path);
    if (!fi.isAbsolute() || !fi.isDir()) {
        return tr("Choose a folder on this computer.");
    }
    const QString clean = QDir::cleanPath(fi.absoluteFilePath());
    if (m_folders.contains(clean)) {
        return {};
    }
    QStringList next = m_folders;
    next << clean;
    return saveFolders(next);
}

QString SettingsLogic::removeIndexFolder(const QString &path)
{
    QStringList next = m_folders;
    next.removeAll(path);
    return saveFolders(next);
}

QString SettingsLogic::folderLabel(const QString &path) const
{
    if (path == QDir::homePath()) {
        return tr("Home");
    }
    return rustDisplayName(path.toUtf8());
}
