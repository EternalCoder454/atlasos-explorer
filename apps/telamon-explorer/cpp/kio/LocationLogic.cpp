#include "LocationLogic.h"

#include "RustBridge.h"

#include <KIO/Global>
#include <KIO/ListJob>

#include <QDir>
#include <QDirListing>
#include <QMetaObject>
#include <QStorageInfo>
#include <QThreadPool>
#include <QTimer>

#include <memory>

namespace
{
// Names read from one folder at most, so a folder of millions can't fill memory.
constexpr int MaxLocalNames = 20000;
constexpr int MaxRemoteNames = 5000;
// How long a server gets to list a folder.
constexpr int RemoteTimeoutMs = 8000;
// A listing made for completion is reused this long.
constexpr qint64 CacheMs = 3000;

enum Flag : char { IsDir = 1, IsHidden = 2 };

QUrl childUrl(const QUrl &dir, const QString &name)
{
    if (dir.isLocalFile()) {
        return QUrl::fromLocalFile(QDir::cleanPath(dir.toLocalFile() + QLatin1Char('/') + name));
    }
    QUrl u = dir;
    u.setPath(QDir::cleanPath(dir.path() + QLatin1Char('/') + name));
    return u;
}

bool hasHiddenChars(const QString &text)
{
    for (const QChar c : text) {
        const ushort u = c.unicode();
        if (c.category() == QChar::Other_Control || u == 0x200E || u == 0x200F || u == 0x061C || (u >= 0x202A && u <= 0x202E) || (u >= 0x2066 && u <= 0x2069)) {
            return true;
        }
    }
    return false;
}
}

LocationLogic::LocationLogic(QObject *parent)
    : QObject(parent)
{
}

int LocationLogic::menuRows() const
{
    return int(telamon_location_limit(0));
}

int LocationLogic::historyRows() const
{
    return int(telamon_location_limit(2));
}

QVariantList LocationLogic::segments(const QUrl &url) const
{
    QVariantList out;
    if (!url.isValid()) {
        return out;
    }
    for (const auto &s : rustPathSegments(url.toString(QUrl::FullyEncoded), QDir::homePath())) {
        out.append(QVariantMap{{QStringLiteral("label"), s.first}, {QStringLiteral("url"), QUrl::fromEncoded(s.second.toUtf8())}});
    }
    return out;
}

QString LocationLogic::editText(const QUrl &url) const
{
    if (!url.isValid()) {
        return QString();
    }
    if (url.isLocalFile()) {
        const QString path = url.toLocalFile();
        if (!hasHiddenChars(path)) {
            return path;
        }
    }
    return url.toString(QUrl::FullyEncoded | QUrl::RemovePassword);
}

QString LocationLogic::sizeText(double bytes) const
{
    return KIO::convertSize(KIO::filesize_t(bytes < 0 ? 0 : bytes));
}

// A local folder is read on a worker; a server's by a KIO job.
void LocationLogic::list(const QUrl &folder, std::function<void(const Listing &)> done)
{
    if (!folder.isLocalFile()) {
        listRemote(folder, std::move(done));
        return;
    }
    QPointer<LocationLogic> self(this);
    const QString path = folder.toLocalFile();
    QThreadPool::globalInstance()->start([self, path, done = std::move(done)] {
        Listing l;
        const QFileInfo info(path);
        if (!info.isDir()) {
            l.error = QObject::tr("This folder can't be read.");
        } else if (!info.isReadable()) {
            l.error = QObject::tr("You don't have permission to open this folder.");
        } else {
            using F = QDirListing::IteratorFlag;
            int n = 0;
            for (const auto &e : QDirListing(path, F::DirsOnly | F::ResolveSymlinks | F::IncludeHidden)) {
                if (++n > MaxLocalNames) {
                    break;
                }
                const QString name = e.fileName();
                char flags = IsDir;
                if (e.isHidden() || name.startsWith(QLatin1Char('.'))) {
                    flags |= IsHidden;
                }
                l.records.append(flags);
                l.records.append(name.toUtf8());
                l.records.append('\0');
                l.names.append(name);
            }
            l.ok = true;
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self, [done, l] { done(l); }, Qt::QueuedConnection);
    });
}

void LocationLogic::listRemote(const QUrl &folder, std::function<void(const Listing &)> done)
{
    // One server listing at a time: a newer request replaces the older.
    if (auto *old = qobject_cast<KJob *>(m_remote)) {
        old->kill(KJob::Quietly);
    }
    KIO::ListJob *job = KIO::listDir(folder, KIO::HideProgressInfo, KIO::ListJob::ListFlag::IncludeHidden);
    m_remote = job;
    auto listing = std::make_shared<Listing>();
    auto finished = std::make_shared<bool>(false);
    auto finish = [listing, finished, done = std::move(done)](const QString &error) {
        if (*finished) {
            return;
        }
        *finished = true;
        listing->error = error;
        listing->ok = error.isEmpty();
        done(*listing);
    };
    connect(job, &KIO::ListJob::entries, this, [listing, job, finish](KIO::Job *, const KIO::UDSEntryList &entries) {
        for (const KIO::UDSEntry &e : entries) {
            const QString name = e.stringValue(KIO::UDSEntry::UDS_NAME);
            if (!e.isDir() || name == QLatin1String(".") || name == QLatin1String("..") || name.isEmpty()) {
                continue;
            }
            if (listing->names.size() >= MaxRemoteNames) {
                job->kill(KJob::Quietly);
                finish(QString());
                return;
            }
            char flags = IsDir;
            if (name.startsWith(QLatin1Char('.')) || e.numberValue(KIO::UDSEntry::UDS_HIDDEN, 0) != 0) {
                flags |= IsHidden;
            }
            listing->records.append(flags);
            listing->records.append(name.toUtf8());
            listing->records.append('\0');
            listing->names.append(name);
        }
    });
    connect(job, &KJob::result, this, [finish](KJob *j) {
        finish(j->error() ? tr("This folder can't be listed: %1").arg(j->errorString()) : QString());
    });
    QTimer::singleShot(RemoteTimeoutMs, job, [job, finish] {
        job->kill(KJob::Quietly);
        finish(tr("The server isn't answering."));
    });
}

int LocationLogic::listSubfolders(const QUrl &folder, bool showHidden)
{
    const int serial = ++m_serial;
    list(folder, [this, serial, folder, showHidden](const Listing &l) {
        QVariantList rows;
        int more = 0;
        if (l.ok) {
            const QList<quint32> order = rustRankNames(1, QString(), l.records, showHidden);
            const int shown = qMin(int(order.size()), menuRows());
            more = int(order.size()) - shown;
            for (int k = 0; k < shown; ++k) {
                const QString &name = l.names.at(int(order.at(k)));
                rows.append(QVariantMap{{QStringLiteral("label"), rustDisplayName(name.toUtf8())}, {QStringLiteral("url"), childUrl(folder, name)}});
            }
        }
        Q_EMIT subfoldersListed(serial, rows, more, l.error);
    });
    return serial;
}

int LocationLogic::complete(const QString &text, const QUrl &current)
{
    const int serial = ++m_serial;
    const RustSplit split = rustSplitForCompletion(text, current.toString(QUrl::FullyEncoded), QDir::homePath());
    if (!split.ok) {
        // Refused text offers nothing; the reason is shown when Enter is pressed.
        QTimer::singleShot(0, this, [this, serial] { Q_EMIT completionsReady(serial, {}, QString()); });
        return serial;
    }
    const QUrl dir = QUrl::fromEncoded(split.dir.toUtf8());
    auto offer = [this, serial, text, prefix = split.prefix](const Listing &l) {
        QStringList texts;
        if (l.ok) {
            for (quint32 i : rustRankNames(0, prefix, l.records, false)) {
                texts.append(rustCompletionText(text, l.names.at(int(i))));
            }
        }
        Q_EMIT completionsReady(serial, texts, l.error);
    };
    if (m_cacheUrl == dir && m_cacheAge.isValid() && m_cacheAge.elapsed() < CacheMs) {
        const Listing cached = m_cache;
        QTimer::singleShot(0, this, [offer, cached] { offer(cached); });
        return serial;
    }
    list(dir, [this, dir, offer](const Listing &l) {
        if (l.ok) {
            m_cacheUrl = dir;
            m_cache = l;
            m_cacheAge.start();
        }
        offer(l);
    });
    return serial;
}

int LocationLogic::queryFreeSpace(const QUrl &folder)
{
    const int serial = ++m_serial;
    if (!folder.isLocalFile()) {
        QTimer::singleShot(0, this, [this, serial] { Q_EMIT freeSpaceReady(serial, -1); });
        return serial;
    }
    QPointer<LocationLogic> self(this);
    const QString path = folder.toLocalFile();
    QThreadPool::globalInstance()->start([self, path, serial] {
        const QStorageInfo info(path);
        const double free = info.isValid() && info.isReady() ? double(info.bytesAvailable()) : -1;
        if (self) {
            QMetaObject::invokeMethod(self, [self, serial, free] { Q_EMIT self->freeSpaceReady(serial, free); }, Qt::QueuedConnection);
        }
    });
    return serial;
}
