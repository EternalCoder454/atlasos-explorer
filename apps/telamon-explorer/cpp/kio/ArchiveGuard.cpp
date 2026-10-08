#include "ArchiveGuard.h"

#include "OpsBridge.h"
#include "RustBridge.h"

#include <KIO/Global>
#include <KIO/ListJob>
#include <KIO/StatJob>

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QSet>
#include <QMetaObject>
#include <QThreadPool>
#include <QTimer>

namespace
{
// A listing this long is not one Files reads into memory to look at.
constexpr qint64 MaxEntries = 1000000;

QString childPath(const QString &parent, const QString &name)
{
    return parent.endsWith(QLatin1Char('/')) ? parent + name : parent + QLatin1Char('/') + name;
}

// Why KIO couldn't read the archive, in plain words (KIO's own text is
// technical and can hold paths).
QString unreadable(const QString &archiveName, bool archiveInstalled)
{
    const QString name = rustDisplayName(archiveName.toUtf8());
    const QString base = QObject::tr("Files can't read \"%1\". It may be damaged, or it may need a password, which Files can't ask for.").arg(name);
    return archiveInstalled ? base + QLatin1Char(' ') + QObject::tr("Open it with Telamon Archive instead.") : base;
}
}

ArchiveGuardJob::ArchiveGuardJob(const QList<QUrl> &sources, bool root, std::shared_ptr<ArchivePlan> plan, bool archiveInstalled, QObject *parent)
    : KJob(parent)
    , m_sources(sources)
    , m_root(root)
    , m_plan(std::move(plan))
    , m_archiveInstalled(archiveInstalled)
{
    QMetaObject::invokeMethod(this, &ArchiveGuardJob::start, Qt::QueuedConnection);
}

void ArchiveGuardJob::start()
{
    // An encrypted zip is looked for first: its listing works, its data doesn't.
    QList<QUrl> files;
    QSet<QString> seen;
    for (const QUrl &u : std::as_const(m_sources)) {
        const RustArchiveLocation where = rustArchiveLocate(u);
        if (where.valid && where.file.isLocalFile() && !seen.contains(where.file.toLocalFile())) {
            seen.insert(where.file.toLocalFile());
            files << where.file;
        }
    }
    QPointer<ArchiveGuardJob> self(this);
    ArchiveCheck::findEncryptedZip(files, this, [self](const QString &name, bool unknown) {
        if (!self || self->m_done) {
            return;
        }
        if (!name.isEmpty()) {
            self->fail(NeedsPassword, unknown ? ArchiveCheck::undecidedText(name, self->m_archiveInstalled) : ArchiveCheck::needsPasswordText(name, self->m_archiveInstalled));
            return;
        }
        self->next();
    });
}

bool ArchiveGuardJob::doKill()
{
    m_done = true;
    if (m_sub) {
        m_sub->kill(KJob::Quietly);
    }
    return true;
}

void ArchiveGuardJob::fail(int code, const QString &text)
{
    if (m_done) {
        return;
    }
    m_done = true;
    // The listing that was running stops with it.
    if (m_sub) {
        m_sub->kill(KJob::Quietly);
    }
    setError(code);
    setErrorText(text);
    emitResult();
}

void ArchiveGuardJob::addRecord(const QString &path, const QString &link, bool isLink, quint64 size)
{
    ++m_count;
    m_bytes += size;
    // Names of a million entries at 64 KiB each are not held in memory.
    if (m_records.size() > qsizetype(256) * 1024 * 1024) {
        m_toobig = true;
        return;
    }
    m_records.append(isLink ? 'l' : 'f');
    m_records.append(path.toUtf8());
    m_records.append('\0');
    if (isLink) {
        m_records.append(link.toUtf8());
        m_records.append('\0');
    }
}

void ArchiveGuardJob::next()
{
    if (m_done) {
        return;
    }
    if (m_at >= m_sources.size()) {
        finishChecks();
        return;
    }
    const QUrl src = m_sources.at(m_at++);
    if (m_root) {
        listed(src, QString(), true);
        return;
    }
    // What is at the top: a file, a folder or a link (a link is looked at, never followed).
    KIO::StatJob *st = KIO::stat(src, KIO::StatJob::SourceSide, KIO::StatBasic, KIO::HideProgressInfo);
    m_sub = st;
    connect(st, &KJob::result, this, [this, st, src] {
        if (m_done) {
            return;
        }
        if (st->error()) {
            fail(KJob::UserDefinedError, unreadable(src.adjusted(QUrl::StripTrailingSlash).fileName(), m_archiveInstalled));
            return;
        }
        const KIO::UDSEntry e = st->statResult();
        const QString top = src.adjusted(QUrl::StripTrailingSlash).fileName();
        const QString link = e.stringValue(KIO::UDSEntry::UDS_LINK_DEST);
        const bool isLink = !link.isEmpty();
        addRecord(top, link, isLink, e.isDir() ? 0 : quint64(e.numberValue(KIO::UDSEntry::UDS_SIZE, 0)));
        if (e.isDir() && !isLink) {
            // The folder's entries, below its name.
            listed(src, top, false);
        } else {
            next();
        }
    });
}

// Lists `src` and everything below it. `top` is the name it is extracted as
// ("" for the top of the archive itself).
void ArchiveGuardJob::listed(const QUrl &src, const QString &top, bool root)
{
    KIO::ListJob *job = KIO::listRecursive(src, KIO::HideProgressInfo);
    m_sub = job;
    connect(job, &KIO::ListJob::entries, this, [this, src, top, root](KIO::Job *, const KIO::UDSEntryList &list) {
        for (const KIO::UDSEntry &e : list) {
            if (m_done) {
                return;
            }
            const QString name = e.stringValue(KIO::UDSEntry::UDS_NAME);
            // The folder itself.
            if (name == QLatin1String(".")) {
                continue;
            }
            if (m_count >= MaxEntries) {
                fail(KJob::UserDefinedError, tr("This archive holds more than a million items, which is more than Files extracts. Open it with Telamon Archive instead."));
                return;
            }
            const QString link = e.stringValue(KIO::UDSEntry::UDS_LINK_DEST);
            const bool isLink = !link.isEmpty();
            const QString path = top.isEmpty() ? name : top + QLatin1Char('/') + name;
            addRecord(path, link, isLink, e.isDir() ? 0 : quint64(e.numberValue(KIO::UDSEntry::UDS_SIZE, 0)));
            // What the top of the archive holds is what is copied out.
            if (root && !name.contains(QLatin1Char('/'))) {
                QUrl c = src;
                c.setPath(childPath(src.path(), name));
                m_plan->children << c;
            }
        }
    });
    connect(job, &KJob::result, this, [this, src, job] {
        if (m_done) {
            return;
        }
        if (job->error()) {
            fail(KJob::UserDefinedError, unreadable(src.adjusted(QUrl::StripTrailingSlash).fileName(), m_archiveInstalled));
            return;
        }
        next();
    });
}

void ArchiveGuardJob::finishChecks()
{
    if (m_done) {
        return;
    }
    if (m_toobig) {
        fail(KJob::UserDefinedError, tr("The list of what is in this archive is too long for Files. Open it with Telamon Archive instead."));
        return;
    }
    const RustArchiveCheck check = rustArchiveCheck(m_records, m_archiveInstalled);
    if (!check.ok) {
        fail(Refused, check.text);
        return;
    }
    m_plan->bytes += m_bytes;
    m_done = true;
    emitResult();
}

// ---- Zip files that need a password ----

namespace
{
enum class ZipState { Plain, Encrypted, Unknown };

bool startsWith(QFile &f, qint64 at, const char *sig)
{
    return f.seek(at) && f.read(4) == QByteArray(sig, 4);
}

ZipState zipState(const QString &path)
{
    QFile f(path);
    if (!f.open(QIODevice::ReadOnly)) {
        return ZipState::Plain;
    }
    const qint64 size = f.size();
    if (size < 22) {
        return ZipState::Plain;
    }
    constexpr qint64 TailBytes = 22 + 65535;
    const qint64 n = qMin(size, TailBytes);
    if (!f.seek(size - n)) {
        return ZipState::Plain;
    }
    const QByteArray tail = f.read(n);
    uint64_t dir[4] = {0, 0, 0, 0};
    int end = telamon_zip_end(reinterpret_cast<const uint8_t *>(tail.constData()), size_t(tail.size()), uint64_t(size), dir);
    if (tail.size() != n) {
        return ZipState::Unknown;
    }
    if (end == 0) {
        // No end record: not a zip, and KIO will say what it makes of the file.
        return ZipState::Plain;
    }
    if (end == 2) {
        // zip64: the end record's own record holds the directory's place.
        const uint64_t record = dir[0];
        if (record > uint64_t(size) || !f.seek(qint64(record))) {
            return ZipState::Unknown;
        }
        const QByteArray rec = f.read(56);
        const uint64_t endAt = uint64_t(size - n);
        if (!telamon_zip64_directory(reinterpret_cast<const uint8_t *>(rec.constData()), size_t(rec.size()), endAt, dir)) {
            return ZipState::Unknown;
        }
        end = 1;
    }
    if (end != 1) {
        return ZipState::Unknown;
    }
    constexpr uint64_t MaxDirectory = 256ull * 1024 * 1024;
    if (dir[1] == 0) {
        return ZipState::Plain;
    }
    if (dir[1] > MaxDirectory) {
        return ZipState::Unknown;
    }
    // Where the directory says it is, or just before the end record (a zip
    // with something in front of it has its offsets moved).
    qint64 at = -1;
    if (dir[0] + dir[1] <= uint64_t(size) && startsWith(f, qint64(dir[0]), "PK\x01\x02")) {
        at = qint64(dir[0]);
    } else if (dir[3] >= dir[1] && startsWith(f, qint64(dir[3] - dir[1]), "PK\x01\x02")) {
        at = qint64(dir[3] - dir[1]);
    }
    if (at < 0 || !f.seek(at)) {
        return ZipState::Unknown;
    }
    const QByteArray list = f.read(qint64(dir[1]));
    if (uint64_t(list.size()) != dir[1]) {
        return ZipState::Unknown;
    }
    return telamon_zip_encrypted(reinterpret_cast<const uint8_t *>(list.constData()), size_t(list.size())) ? ZipState::Encrypted : ZipState::Plain;
}
}

void ArchiveCheck::findEncryptedZip(const QList<QUrl> &files, QObject *context, std::function<void(const QString &, bool)> done)
{
    QStringList paths;
    for (const QUrl &u : files) {
        if (u.isLocalFile()) {
            paths << u.toLocalFile();
        }
    }
    if (paths.isEmpty()) {
        QTimer::singleShot(0, context, [done] { done(QString(), false); });
        return;
    }
    QPointer<QObject> guard(context);
    QThreadPool::globalInstance()->start([guard, paths, done] {
        QString found;
        bool unknown = false;
        for (const QString &p : paths) {
            const ZipState state = zipState(p);
            if (state != ZipState::Plain) {
                found = QFileInfo(p).fileName();
                unknown = state == ZipState::Unknown;
                break;
            }
        }
        if (!guard) {
            return;
        }
        QMetaObject::invokeMethod(guard.data(), [guard, found, unknown, done] {
            if (guard) {
                done(found, unknown);
            }
        });
    });
}

QString ArchiveCheck::needsPasswordText(const QString &name, bool archiveInstalled)
{
    const QString shown = rustDisplayName(name.toUtf8());
    const QString base = QObject::tr("\"%1\" needs a password, which Files can't enter.").arg(shown);
    return archiveInstalled ? base + QLatin1Char(' ') + QObject::tr("Use Extract Here or Extract To in the right-click menu: Telamon Archive asks for it.") : base;
}

QString ArchiveCheck::undecidedText(const QString &name, bool archiveInstalled)
{
    const QString base = QObject::tr("Files can't tell whether \"%1\" needs a password.").arg(rustDisplayName(name.toUtf8()));
    return archiveInstalled ? base + QLatin1Char(' ') + QObject::tr("Use Extract Here or Extract To in the right-click menu: Telamon Archive can open it.") : base;
}

// ---- Where it goes ----

ArchivePrepareJob::ArchivePrepareJob(const QUrl &parent, const QString &name, const QString &what, std::shared_ptr<ArchivePlan> plan, QObject *parent_)
    : KJob(parent_)
    , m_parent(parent)
    , m_name(name)
    , m_what(what)
    , m_plan(std::move(plan))
{
    QMetaObject::invokeMethod(this, &ArchivePrepareJob::start, Qt::QueuedConnection);
}

void ArchivePrepareJob::start()
{
    // A server has no honest answer to "is there room": the job will say.
    if (!m_parent.isLocalFile()) {
        if (!m_name.isEmpty()) {
            QUrl u = m_parent;
            u.setPath(childPath(m_parent.path(), m_name));
            m_plan->dest = u;
        } else {
            m_plan->dest = m_parent;
        }
        emitResult();
        return;
    }
    const QString parentPath = m_parent.toLocalFile();
    const QString name = m_name;
    const QString what = m_what;
    const quint64 bytes = m_plan->bytes;
    QPointer<ArchivePrepareJob> self(this);
    QThreadPool::globalInstance()->start([self, parentPath, name, what, bytes] {
        QString chosen = name;
        if (!name.isEmpty()) {
            chosen = rustFreeName(name, [&](const QString &n) {
                const QFileInfo fi(childPath(parentPath, n));
                return fi.exists() || fi.isSymLink();
            });
        }
        const QString destPath = chosen.isEmpty() ? parentPath : childPath(parentPath, chosen);
        QString why;
        if (bytes > 0) {
            const QByteArray dest = destPath.toUtf8(), w = what.toUtf8();
            size_t len = 0;
            QByteArray out(512, 0);
            auto call = [&] {
                return telamon_room_check(bytes, OpsBridge::p(dest), OpsBridge::n(dest), OpsBridge::p(w), OpsBridge::n(w), -1, reinterpret_cast<uint8_t *>(out.data()),
                                          size_t(out.size()), &len);
            };
            int rc = call();
            if (len > size_t(out.size())) {
                out.resize(qsizetype(len));
                rc = call();
            }
            if (rc != 0) {
                why = QString::fromUtf8(out.constData(), qsizetype(qMin(len, size_t(out.size()))));
            }
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, destPath, why] {
            if (!self) {
                return;
            }
            self->m_plan->dest = QUrl::fromLocalFile(destPath);
            if (!why.isEmpty()) {
                self->setError(NoRoom);
                self->setErrorText(why);
            }
            self->emitResult();
        });
    });
}

NoopJob::NoopJob(QObject *parent)
    : KJob(parent)
{
    QMetaObject::invokeMethod(this, [this] { emitResult(); }, Qt::QueuedConnection);
}
