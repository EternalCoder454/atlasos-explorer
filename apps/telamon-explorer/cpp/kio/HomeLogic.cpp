#include "HomeLogic.h"
#include "PlacesLogic.h"
#include "RustBridge.h"

#include <KConfigGroup>
#include <KSharedConfig>

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QLocale>
#include <QMimeDatabase>
#include <QPointer>
#include <QStandardPaths>
#include <QThreadPool>

namespace
{
constexpr uint32_t LimitFolders = 0, LimitShown = 1, LimitRecent = 3;

KConfigGroup homeGroup()
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("Home"));
}

// Where a location is, as the search results' Path column says it
// ("Home > Documents"): the folder above it, made safe to show.
QString parentText(const QUrl &url)
{
    const QUrl parent = url.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash);
    const QString text = rustSearchPathText(parent.toString(QUrl::FullyEncoded | QUrl::RemovePassword), QDir::homePath());
    // A folder straight in the home folder needs no line saying so.
    return text == QLatin1String("~") ? QString() : text;
}

QVariantMap item(const QString &name, const QUrl &url, const QString &path, const QString &icon, bool isDir, const QString &tip)
{
    return {{QStringLiteral("name"), name},
            {QStringLiteral("url"), url},
            {QStringLiteral("path"), path},
            {QStringLiteral("iconName"), icon},
            {QStringLiteral("isDir"), isDir},
            {QStringLiteral("tip"), tip}};
}

// The name a folder is listed under: its last part, else the server's name.
QString folderName(const QUrl &url)
{
    const QStringList parts = url.path().split(QLatin1Char('/'), Qt::SkipEmptyParts);
    if (!parts.isEmpty()) {
        return rustDisplayName(parts.last().toUtf8());
    }
    if (!url.isLocalFile() && !url.host().isEmpty()) {
        return rustDisplayName(url.host().toUtf8());
    }
    return QStringLiteral("/");
}
}

HomeLogic::HomeLogic(QObject *parent)
    : QObject(parent)
{
    // A settings file can hold anything: the core brings it into its limits.
    m_counts = homeGroup().readEntry("Frequent", QStringList()).join(QLatin1Char('\n')).toUtf8();
    m_timer.setSingleShot(true);
    m_timer.setInterval(800);
    connect(&m_timer, &QTimer::timeout, this, &HomeLogic::flush);
    connect(qApp, &QCoreApplication::aboutToQuit, this, &HomeLogic::flush);
    connect(PlacesLogic::instance(), &PlacesLogic::entriesChanged, this, &HomeLogic::rebuildPinned);
    rebuildPinned();
}

HomeLogic::~HomeLogic()
{
    flush();
}

void HomeLogic::setLoading(bool on)
{
    if (m_loading != on) {
        m_loading = on;
        Q_EMIT loadingChanged();
    }
}

// The folders of the sidebar's own list (the standard ones and the pins),
// without the Home place itself: Home is this page. The Home folder is listed
// as "Home Folder", so the way to the files in it is always on the page.
void HomeLogic::rebuildPinned()
{
    QVariantList out;
    const QUrl homeFolder = QUrl::fromLocalFile(QDir::homePath());
    out.append(item(tr("Home Folder"), homeFolder, QString(), QStringLiteral("user-home"), true, tr("Your files, in %1").arg(rustDisplayName(QDir::homePath().toUtf8()))));
    for (const PlaceEntry &e : PlacesLogic::instance()->entries()) {
        if (e.section != PlacesLogic::Favourites || e.kind != PlacesLogic::Folder || e.hidden || !e.url.isValid()) {
            continue;
        }
        if (PlacesLogic::instance()->sameLocation(e.url, homeFolder)) {
            continue;
        }
        out.append(item(e.text, e.url, parentText(e.url), e.iconName.isEmpty() ? QStringLiteral("folder") : e.iconName, true, e.tooltip));
    }
    if (out != m_pinned) {
        m_pinned = out;
        Q_EMIT pinnedChanged();
    }
}

void HomeLogic::refresh()
{
    rebuildPinned();
    const int serial = ++m_serial;
    setLoading(true);
    const QByteArray counts = m_counts;
    const QString xbel = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) + QStringLiteral("/recently-used.xbel");
    const QString home = QDir::homePath();
    QPointer<HomeLogic> self(this);
    // Reading the recent list and checking that things are still there are
    // stat calls: never on the GUI thread.
    QThreadPool::globalInstance()->start([self, serial, counts, xbel, home] {
        QVariantList recent;
        QVariantList frequent;
        QStringList gone;
        QMimeDatabase mime;

        const QByteArray path = xbel.toUtf8();
        QByteArray buf(8192, 0);
        const size_t cap = telamon_home_limit(LimitRecent);
        size_t n = telamon_home_recent_files(reinterpret_cast<const uint8_t *>(path.constData()), size_t(path.size()), cap, reinterpret_cast<uint8_t *>(buf.data()),
                                             size_t(buf.size()));
        if (n > size_t(buf.size())) {
            buf.resize(qsizetype(n));
            n = telamon_home_recent_files(reinterpret_cast<const uint8_t *>(path.constData()), size_t(path.size()), cap, reinterpret_cast<uint8_t *>(buf.data()),
                                          size_t(buf.size()));
        }
        for (const QByteArray &line : buf.left(qsizetype(n)).split('\n')) {
            const int tab = line.indexOf('\t');
            if (tab <= 0) {
                continue;
            }
            const qint64 secs = line.left(tab).toLongLong();
            const QUrl url = QUrl::fromEncoded(line.mid(tab + 1));
            if (!url.isLocalFile()) {
                continue;
            }
            const QString file = url.toLocalFile();
            const QString name = rustDisplayName(QFileInfo(file).fileName().toUtf8());
            const QMimeType mt = mime.mimeTypeForFile(file, QMimeDatabase::MatchExtension);
            const QString when = QLocale().toString(QDateTime::fromSecsSinceEpoch(secs), QLocale::ShortFormat);
            recent.append(item(name, url, parentText(url), mt.iconName().isEmpty() ? QStringLiteral("application-octet-stream") : mt.iconName(), false,
                               tr("Used %1").arg(when)));
        }

        QByteArray top(8192, 0);
        size_t t = telamon_home_top(reinterpret_cast<const uint8_t *>(counts.constData()), size_t(counts.size()), telamon_home_limit(LimitFolders), reinterpret_cast<uint8_t *>(top.data()),
                                    size_t(top.size()));
        if (t > size_t(top.size())) {
            top.resize(qsizetype(t));
            t = telamon_home_top(reinterpret_cast<const uint8_t *>(counts.constData()), size_t(counts.size()), telamon_home_limit(LimitFolders), reinterpret_cast<uint8_t *>(top.data()),
                                 size_t(top.size()));
        }
        const int shown = int(telamon_home_limit(LimitShown));
        for (const QByteArray &line : top.left(qsizetype(t)).split('\n')) {
            const int tab = line.indexOf('\t');
            if (tab <= 0 || frequent.size() >= shown) {
                continue;
            }
            const int count = line.left(tab).toInt();
            const QByteArray key = line.mid(tab + 1);
            const QUrl url = QUrl::fromEncoded(key);
            if (url.isLocalFile() && !QFileInfo(url.toLocalFile()).isDir()) {
                // The folder is gone (or was never one): it is forgotten.
                gone << QString::fromUtf8(key);
                continue;
            }
            const bool local = url.isLocalFile();
            const QString note = tr("Opened %n time(s)", "", count);
            frequent.append(item(folderName(url), url, local ? parentText(url) : rustDisplayName(url.toString(QUrl::PrettyDecoded | QUrl::RemovePassword).toUtf8()),
                                 local ? QStringLiteral("folder") : QStringLiteral("folder-remote"), true, note));
        }
        QMetaObject::invokeMethod(
            self.data(),
            [self, serial, recent, frequent, gone] {
                if (!self || serial != self->m_serial) {
                    return;
                }
                for (const QString &g : gone) {
                    const QByteArray k = g.toUtf8();
                    self->m_counts = rustBytes2(telamon_home_forget, self->m_counts, k);
                    self->m_dirty = true;
                }
                if (!gone.isEmpty()) {
                    self->m_timer.start();
                    Q_EMIT self->countedChanged();
                }
                if (recent != self->m_recent) {
                    self->m_recent = recent;
                    Q_EMIT self->recentChanged();
                }
                if (frequent != self->m_frequent) {
                    self->m_frequent = frequent;
                    Q_EMIT self->frequentChanged();
                }
                self->setLoading(false);
            },
            Qt::QueuedConnection);
    });
}

void HomeLogic::visited(const QUrl &folder)
{
    if (!folder.isValid()) {
        return;
    }
    QByteArray in = folder.toString(QUrl::FullyEncoded | QUrl::RemovePassword).toUtf8();
    in.append('\0');
    in.append(QDir::homePath().toUtf8());
    const QByteArray key = rustBytes(telamon_home_key, in);
    if (key.isEmpty()) {
        return;
    }
    QByteArray out(m_counts.size() + 512, 0);
    const auto call = [&] {
        return telamon_home_visit(reinterpret_cast<const uint8_t *>(m_counts.constData()), size_t(m_counts.size()), reinterpret_cast<const uint8_t *>(key.constData()), size_t(key.size()),
                                  QDateTime::currentSecsSinceEpoch(), reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
    };
    size_t n = call();
    if (n > size_t(out.size())) {
        out.resize(qsizetype(n));
        n = call();
    }
    out.truncate(qsizetype(n));
    if (out != m_counts) {
        m_counts = out;
        m_dirty = true;
        m_timer.start();
        Q_EMIT countedChanged();
    }
}

void HomeLogic::clearFrequent()
{
    // A read that is under way is let go.
    ++m_serial;
    setLoading(false);
    m_counts.clear();
    m_dirty = true;
    flush();
    Q_EMIT countedChanged();
    if (!m_frequent.isEmpty()) {
        m_frequent.clear();
        Q_EMIT frequentChanged();
    }
}

int HomeLogic::countedFolders() const
{
    return m_counts.isEmpty() ? 0 : int(m_counts.count('\n'));
}

void HomeLogic::flush()
{
    m_timer.stop();
    if (!m_dirty) {
        return;
    }
    m_dirty = false;
    KConfigGroup g = homeGroup();
    QStringList lines;
    for (const QByteArray &l : m_counts.split('\n')) {
        if (!l.isEmpty()) {
            lines << QString::fromUtf8(l);
        }
    }
    if (lines.isEmpty()) {
        // Cleared means gone from the file too.
        g.deleteEntry("Frequent");
    } else {
        g.writeEntry("Frequent", lines);
    }
    g.sync();
}

bool HomeLogic::sectionOpen(const QString &name) const
{
    return !homeGroup().readEntry("Folded", QStringList()).contains(name);
}

void HomeLogic::setSectionOpen(const QString &name, bool open)
{
    KConfigGroup g = homeGroup();
    QStringList folded = g.readEntry("Folded", QStringList());
    if (open) {
        folded.removeAll(name);
    } else if (!folded.contains(name)) {
        folded << name;
    }
    if (folded.isEmpty()) {
        g.deleteEntry("Folded");
    } else {
        g.writeEntry("Folded", folded);
    }
    g.sync();
}
