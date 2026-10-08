#include "PropertiesLogic.h"

#include "MetaReader.h"
#include "PropsBridge.h"
#include "RustBridge.h"
#include "TagLogic.h"

#include <KApplicationTrader>
#include <KConfig>
#include <KConfigGroup>
#include <KFileItem>
#include <KIO/DirectorySizeJob>
#include <KIO/Global>
#include <KIO/StatJob>
#include <KService>

#include <QDir>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QLocale>
#include <QMimeDatabase>
#include <QStandardPaths>
#include <QThreadPool>

#include <sys/stat.h>

namespace
{
constexpr int MaxItems = 64;

QString dateText(qint64 secs)
{
    return secs > 0 ? QLocale().toString(QDateTime::fromSecsSinceEpoch(secs), QLocale::LongFormat) : QString();
}

// "1 file", "2 files": Qt's %n has no plural form to use without translations.
QString counted(qint64 n, const char *one, const char *many)
{
    return QStringLiteral("%1 %2").arg(QLocale().toString(n), QString::fromLatin1(n == 1 ? one : many));
}

QString shown(const QString &s)
{
    return rustDisplayName(s.toUtf8());
}

// Where the item is, written for a person: a path under the home folder as `~/...`, a server's address without its password.
QString parentText(const QUrl &url)
{
    const QUrl parent = url.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash);
    if (parent.isLocalFile()) {
        return rustSearchPathText(parent.toString(QUrl::FullyEncoded), QDir::homePath());
    }
    return shown(parent.toString(QUrl::RemovePassword | QUrl::PreferLocalFile));
}

// The worker side: everything about one local item.
PropertiesLogic::Item readLocal(const QUrl &url)
{
    PropertiesLogic::Item it;
    it.url = url;
    it.local = true;
    const QString path = url.toLocalFile();
    const QByteArray enc = QFile::encodeName(path);
    struct stat st;
    if (::lstat(enc.constData(), &st) != 0) {
        return it;
    }
    it.ok = true;
    const QFileInfo fi(path);
    it.name = path == QLatin1String("/") ? path : fi.fileName();
    it.isLink = S_ISLNK(st.st_mode);
    it.isDir = fi.isDir();
    if (it.isLink) {
        it.linkTarget = fi.symLinkTarget();
        if (it.linkTarget.isEmpty()) {
            it.linkTarget = QFile::decodeName(QFile::symLinkTarget(path).toUtf8());
        }
    }
    it.size = it.isDir ? 0 : quint64(fi.size() < 0 ? 0 : fi.size());
    it.mtime = fi.lastModified().toSecsSinceEpoch();
    const QDateTime born = fi.birthTime();
    it.ctime = born.isValid() ? born.toSecsSinceEpoch() : 0;
    it.atime = fi.lastRead().toSecsSinceEpoch();
    it.mode = uint(st.st_mode) & 07777;
    it.uid = uint(st.st_uid);
    it.owner = fi.owner();
    it.group = fi.group();
    const QMimeType mt = QMimeDatabase().mimeTypeForFile(fi, QMimeDatabase::MatchDefault);
    it.mime = mt.name();
    it.kind = it.isDir ? QObject::tr("Folder") : (mt.isDefault() ? QObject::tr("File") : mt.comment());
    it.icon = it.isDir ? QStringLiteral("folder") : (mt.iconName().isEmpty() ? QStringLiteral("application-octet-stream") : mt.iconName());
    if (!it.isLink) {
        const PropsBridge::TagsRead r = PropsBridge::readTags(path);
        it.tags = r.names;
        it.tagStatus = int(r.status);
        it.rating = telamon_rating_read(PropsBridge::p(enc), PropsBridge::n(enc));
    } else {
        it.tagStatus = 2;
    }
    return it;
}

// What a progress callback from the checksum worker knows.
struct SumCtx {
    QPointer<PropertiesLogic> logic;
    quint64 gen;
    quint64 total;
    QElapsedTimer since;
    qint64 last = -1000;
};
}

PropertiesLogic::PropertiesLogic(QObject *parent)
    : QObject(parent)
{
}

PropertiesLogic::~PropertiesLogic()
{
    stopAll();
}

void PropertiesLogic::stopAll()
{
    if (m_sumCancel) {
        *m_sumCancel = true;
    }
    if (m_sizeCancel) {
        *m_sizeCancel = true;
    }
    if (m_sizeJob) {
        m_sizeJob->kill(KJob::Quietly);
    }
    m_sumRunning = false;
    m_sizeRunning = false;
}

// ---- Loading ----

void PropertiesLogic::load(const QList<QUrl> &urls)
{
    stopAll();
    ++m_gen;
    m_urls = urls.mid(0, MaxItems);
    m_items.clear();
    m_sums.clear();
    m_expected.clear();
    m_sumError.clear();
    m_sumProgress = 0;
    m_pendingCompareAlg = -1;
    m_sizeKnown = false;
    m_sizeBytes = m_sizeFiles = m_sizeFolders = m_sizeOnDisk = 0;
    m_details.clear();
    m_detailsLoaded = false;
    m_general = {};
    m_perms = {};
    m_openWith = {};
    m_tagsInfo = {};
    m_rating = -1;
    m_ratingAvailable = false;
    m_checksum = {{QStringLiteral("available"), false}, {QStringLiteral("running"), false}, {QStringLiteral("progress"), 0.0}, {QStringLiteral("results"), QVariantList()}, {QStringLiteral("compare"), QVariantMap{{QStringLiteral("state"), QString()}, {QStringLiteral("text"), QString()}}}};
    m_folderSize = {{QStringLiteral("available"), false}, {QStringLiteral("running"), false}, {QStringLiteral("state"), QStringLiteral("idle")}, {QStringLiteral("text"), QString()}};
    m_items.resize(m_urls.size());
    m_busy = !m_urls.isEmpty();
    Q_EMIT urlsChanged();
    Q_EMIT busyChanged();
    Q_EMIT generalChanged();
    Q_EMIT permsChanged();
    Q_EMIT detailsChanged();
    Q_EMIT openWithChanged();
    Q_EMIT attributesChanged();
    Q_EMIT checksumChanged();
    Q_EMIT folderSizeChanged();
    if (m_urls.isEmpty()) {
        return;
    }

    // Items on this computer are read together by one worker; each item on a
    // server by a KIO stat job.
    QList<QPair<int, QUrl>> local;
    QList<QPair<int, QUrl>> remote;
    for (int i = 0; i < m_urls.size(); ++i) {
        (m_urls.at(i).isLocalFile() ? local : remote).append({i, m_urls.at(i)});
    }
    m_pending = (local.isEmpty() ? 0 : 1) + int(remote.size());
    const quint64 gen = m_gen;
    if (!local.isEmpty()) {
        QPointer<PropertiesLogic> self(this);
        QThreadPool::globalInstance()->start([self, gen, local] {
            QList<QPair<int, Item>> found;
            for (const auto &p : local) {
                found.append({p.first, readLocal(p.second)});
            }
            if (!self) {
                return;
            }
            QMetaObject::invokeMethod(self.data(), [self, gen, found] {
                if (!self || gen != self->m_gen) {
                    return;
                }
                for (const auto &p : found) {
                    self->m_items[p.first] = p.second;
                }
                if (--self->m_pending == 0) {
                    self->assemble();
                }
            });
        });
    }
    for (const auto &p : remote) {
        KIO::StatJob *job = KIO::stat(p.second, KIO::StatJob::SourceSide, KIO::StatDefaultDetails, KIO::HideProgressInfo);
        const int index = p.first;
        const QUrl url = p.second;
        connect(job, &KJob::result, this, [this, gen, index, url, job] {
            if (gen != m_gen) {
                return;
            }
            Item it;
            it.url = url;
            if (!job->error()) {
                const KIO::UDSEntry e = job->statResult();
                it.ok = true;
                it.name = e.stringValue(KIO::UDSEntry::UDS_NAME);
                if (it.name.isEmpty()) {
                    it.name = url.adjusted(QUrl::StripTrailingSlash).fileName();
                }
                it.isDir = e.isDir();
                it.isLink = e.isLink();
                it.linkTarget = e.stringValue(KIO::UDSEntry::UDS_LINK_DEST);
                it.size = it.isDir ? 0 : quint64(e.numberValue(KIO::UDSEntry::UDS_SIZE, 0));
                it.mtime = e.numberValue(KIO::UDSEntry::UDS_MODIFICATION_TIME, 0);
                it.ctime = e.numberValue(KIO::UDSEntry::UDS_CREATION_TIME, 0);
                it.atime = e.numberValue(KIO::UDSEntry::UDS_ACCESS_TIME, 0);
                it.owner = e.stringValue(KIO::UDSEntry::UDS_USER);
                it.group = e.stringValue(KIO::UDSEntry::UDS_GROUP);
                it.mode = uint(e.numberValue(KIO::UDSEntry::UDS_ACCESS, 0));
                const QMimeType mt = QMimeDatabase().mimeTypeForFile(it.name, QMimeDatabase::MatchExtension);
                it.mime = it.isDir ? QStringLiteral("inode/directory") : mt.name();
                it.kind = it.isDir ? tr("Folder") : (mt.isDefault() ? tr("File") : mt.comment());
                it.icon = it.isDir ? QStringLiteral("folder") : (mt.iconName().isEmpty() ? QStringLiteral("application-octet-stream") : mt.iconName());
                it.tagStatus = 1;
            }
            m_items[index] = it;
            if (--m_pending == 0) {
                assemble();
            }
        });
    }
}

void PropertiesLogic::reloadAttributes()
{
    // The same read as the first time, for the items on this computer only.
    QList<QPair<int, QUrl>> local;
    for (int i = 0; i < m_urls.size(); ++i) {
        if (m_urls.at(i).isLocalFile()) {
            local.append({i, m_urls.at(i)});
        }
    }
    if (local.isEmpty()) {
        return;
    }
    const quint64 gen = m_gen;
    QPointer<PropertiesLogic> self(this);
    QThreadPool::globalInstance()->start([self, gen, local] {
        QList<QPair<int, Item>> found;
        for (const auto &p : local) {
            found.append({p.first, readLocal(p.second)});
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, gen, found] {
            if (!self || gen != self->m_gen) {
                return;
            }
            for (const auto &p : found) {
                if (p.first < self->m_items.size()) {
                    self->m_items[p.first] = p.second;
                }
            }
            self->assemble(false);
            self->assembleAttributes();
        });
    });
}

QString PropertiesLogic::sizeLine(quint64 bytes) const
{
    if (bytes < 1024) {
        return tr("%1 bytes").arg(QLocale().toString(bytes));
    }
    return tr("%1 (%2 bytes)").arg(KIO::convertSize(KIO::filesize_t(bytes)), QLocale().toString(bytes));
}

// ---- Putting the facts together ----

void PropertiesLogic::assemble(bool full)
{
    m_busy = false;
    Q_EMIT busyChanged();
    QList<const Item *> items;
    for (const Item &it : std::as_const(m_items)) {
        if (it.ok) {
            items.append(&it);
        }
    }
    QVariantMap g;
    if (items.isEmpty()) {
        g = {{QStringLiteral("gone"), true},
             {QStringLiteral("title"), tr("Not Found")},
             {QStringLiteral("single"), true},
             {QStringLiteral("count"), 0},
             {QStringLiteral("iconName"), QStringLiteral("dialog-warning")},
             {QStringLiteral("kind"), tr("This item isn't there any more, or it can't be reached.")}};
        m_general = g;
        Q_EMIT generalChanged();
        return;
    }
    const bool single = items.size() == 1;
    const Item &first = *items.first();
    int dirs = 0, files = 0;
    bool allLocal = true;
    quint64 fileBytes = 0;
    for (const Item *it : items) {
        (it->isDir ? dirs : files)++;
        allLocal = allLocal && it->local;
        if (!it->isDir) {
            fileBytes += it->size;
        }
    }
    g[QStringLiteral("gone")] = false;
    g[QStringLiteral("single")] = single;
    g[QStringLiteral("count")] = int(items.size());
    g[QStringLiteral("local")] = allLocal;
    g[QStringLiteral("anyDir")] = dirs > 0;
    g[QStringLiteral("onlyFolders")] = files == 0;
    g[QStringLiteral("title")] = single ? shown(first.name) : counted(items.size(), "item", "items");
    g[QStringLiteral("iconName")] = single ? first.icon : QStringLiteral("document-multiple");
    g[QStringLiteral("name")] = single ? first.name : QString();
    // A name can be changed where Files can rename (not in the Trash, not in an archive).
    const QString scheme = first.url.scheme();
    g[QStringLiteral("nameEditable")] = single && scheme != QLatin1String("trash") && !rustArchiveScheme(scheme);
    if (single) {
        QString kind = first.kind;
        if (first.isLink) {
            kind = tr("%1 (link)").arg(first.kind);
            g[QStringLiteral("linkTarget")] = shown(first.linkTarget);
        }
        g[QStringLiteral("kind")] = kind;
        g[QStringLiteral("mime")] = first.mime;
    } else {
        QStringList parts;
        if (dirs > 0) {
            parts << counted(dirs, "folder", "folders");
        }
        if (files > 0) {
            parts << counted(files, "file", "files");
        }
        g[QStringLiteral("kind")] = parts.join(QStringLiteral(", "));
    }
    // Where: one place, or "several".
    QString where = parentText(first.url);
    for (const Item *it : items) {
        if (parentText(it->url) != where) {
            where = tr("Several locations");
            break;
        }
    }
    g[QStringLiteral("location")] = where;
    // Size.
    QString sizeText;
    QString note;
    if (single && !first.isDir) {
        sizeText = sizeLine(first.size);
    } else if (dirs == 0) {
        sizeText = sizeLine(fileBytes);
    } else if (m_sizeKnown) {
        sizeText = sizeLine(fileBytes + m_sizeBytes);
        note = tr("Contains %1 and %2. Takes %3 on the disk.").arg(counted(qint64(m_sizeFiles), "file", "files"), counted(qint64(m_sizeFolders), "folder", "folders"), KIO::convertSize(KIO::filesize_t(m_sizeOnDisk)));
    } else {
        sizeText = files > 0 ? tr("%1, not counting the folders").arg(sizeLine(fileBytes)) : tr("Not counted yet");
    }
    g[QStringLiteral("sizeText")] = sizeText;
    g[QStringLiteral("sizeNote")] = note;
    if (single) {
        g[QStringLiteral("created")] = dateText(first.ctime);
        g[QStringLiteral("modified")] = dateText(first.mtime);
        g[QStringLiteral("accessed")] = dateText(first.atime);
    } else {
        qint64 newest = 0;
        for (const Item *it : items) {
            newest = qMax(newest, it->mtime);
        }
        g[QStringLiteral("modified")] = dateText(newest);
    }
    m_general = g;
    Q_EMIT generalChanged();
    if (!full) {
        return;
    }

    // Folder size is offered when a folder is among the items.
    if (dirs > 0 && !m_sizeRunning) {
        m_folderSize[QStringLiteral("available")] = true;
        Q_EMIT folderSizeChanged();
    }

    // Open With: one kind of file (not folders).
    {
        QVariantMap ow{{QStringLiteral("available"), false}};
        QSet<QString> mimes;
        for (const Item *it : items) {
            mimes.insert(it->mime);
        }
        if (dirs > 0) {
            ow[QStringLiteral("why")] = tr("Folders open in Files.");
        } else if (mimes.size() != 1) {
            ow[QStringLiteral("why")] = tr("Select items of one kind to choose what opens them.");
        } else {
            const QString mime = *mimes.begin();
            const KService::Ptr preferred = KApplicationTrader::preferredService(mime);
            QVariantList apps;
            const KService::List all = KApplicationTrader::queryByMimeType(mime);
            for (const KService::Ptr &s : all) {
                if (apps.size() >= 40) {
                    break;
                }
                apps.append(QVariantMap{{QStringLiteral("id"), s->storageId()}, {QStringLiteral("name"), shown(s->name())}, {QStringLiteral("icon"), s->icon()}});
            }
            ow[QStringLiteral("available")] = !apps.isEmpty();
            ow[QStringLiteral("why")] = apps.isEmpty() ? tr("No application is known for this kind of file.") : QString();
            ow[QStringLiteral("mime")] = mime;
            ow[QStringLiteral("apps")] = apps;
            ow[QStringLiteral("kind")] = first.kind;
            if (preferred) {
                ow[QStringLiteral("current")] = QVariantMap{{QStringLiteral("id"), preferred->storageId()}, {QStringLiteral("name"), shown(preferred->name())}, {QStringLiteral("icon"), preferred->icon()}};
            }
        }
        m_openWith = ow;
        Q_EMIT openWithChanged();
    }

    // Checksums: one file on this computer.
    {
        const bool can = single && first.local && !first.isDir;
        m_checksum[QStringLiteral("available")] = can;
        m_checksum[QStringLiteral("why")] = can ? QString() : (single && first.isDir ? tr("Folders don't have a checksum. Open one file to see its checksum.") : (!allLocal ? tr("Checksums are for files on this computer.") : tr("Select one file to see its checksum.")));
        Q_EMIT checksumChanged();
    }

    assembleAttributes();
    startDetails();
}

// Permissions, tags and rating.
void PropertiesLogic::assembleAttributes()
{
    QList<const Item *> items;
    for (const Item &it : std::as_const(m_items)) {
        if (it.ok) {
            items.append(&it);
        }
    }
    if (items.isEmpty()) {
        return;
    }
    const bool single = items.size() == 1;
    const Item &first = *items.first();
    bool allLocal = true, anyLink = false, mine = true, anyDir = false, anyFile = false;
    for (const Item *it : items) {
        allLocal = allLocal && it->local;
        anyLink = anyLink || it->isLink;
        mine = mine && it->local && it->uid == telamon_current_uid();
        anyDir = anyDir || it->isDir;
        anyFile = anyFile || !it->isDir;
    }

    // Permissions.
    QVariantMap p;
    const bool available = allLocal && !anyLink;
    p[QStringLiteral("available")] = available;
    p[QStringLiteral("editable")] = available && mine;
    p[QStringLiteral("anyDir")] = anyDir;
    if (!allLocal) {
        p[QStringLiteral("why")] = tr("Permissions can only be shown for items on this computer.");
    } else if (anyLink) {
        p[QStringLiteral("why")] = tr("A link uses the permissions of what it points to.");
    } else if (!mine) {
        p[QStringLiteral("why")] = tr("Only the owner can change these.");
    }
    if (available) {
        QVariantList rows;
        const QStringList labels{tr("Owner"), tr("Group"), tr("Everyone else")};
        for (uint who = 0; who < 3; ++who) {
            QVariantMap row;
            QString label = labels.at(int(who));
            if (single && who == 0 && !first.owner.isEmpty()) {
                label = tr("Owner (%1)").arg(shown(first.owner));
            } else if (single && who == 1 && !first.group.isEmpty()) {
                label = tr("Group (%1)").arg(shown(first.group));
            }
            row[QStringLiteral("who")] = who;
            row[QStringLiteral("label")] = label;
            const char *keys[] = {"read", "write", "run"};
            for (uint bit = 0; bit < 3; ++bit) {
                int on = 0;
                for (const Item *it : items) {
                    on += (telamon_perm_access(it->mode, who) >> bit) & 1;
                }
                row[QLatin1String(keys[bit])] = on == 0 ? 0 : (on == items.size() ? 2 : 1);
            }
            if (single) {
                row[QStringLiteral("words")] = PropsBridge::textOf(0, first.mode, who, first.isDir);
            }
            rows.append(row);
        }
        p[QStringLiteral("rows")] = rows;
        p[QStringLiteral("runLabel")] = anyDir && anyFile ? tr("Run or Open") : (anyDir ? tr("Open") : tr("Run"));
        p[QStringLiteral("canRecurse")] = anyDir && mine;
        p[QStringLiteral("owner")] = single ? shown(first.owner) : QString();
        p[QStringLiteral("group")] = single ? shown(first.group) : QString();
        if (single) {
            p[QStringLiteral("sentence")] = PropsBridge::textOf(1, first.mode, mine ? 1 : 0, first.isDir);
            p[QStringLiteral("octal")] = PropsBridge::textOf(2, first.mode, 0, false);
            p[QStringLiteral("symbolic")] = PropsBridge::textOf(3, first.mode, 0, false);
        }
    }
    m_perms = p;
    Q_EMIT permsChanged();

    // Tags and rating.
    QVariantMap t;
    QString why;
    QList<QStringList> lists;
    for (const Item *it : items) {
        lists << it->tags;
        if (!it->local || it->tagStatus == 1) {
            why = tr("This location can't keep tags.");
        } else if (why.isEmpty() && it->tagStatus != 0) {
            why = QString::fromUtf8(PropsBridge::bytesOf([&](uint8_t *o, size_t c) { return telamon_tags_status_text(uint32_t(it->tagStatus), o, c); }));
        }
    }
    QByteArray packed;
    QStringList names;
    for (qsizetype i = 0; i < lists.size(); ++i) {
        if (i > 0) {
            packed.append('\x1e');
        }
        packed += lists.at(i).join(QLatin1Char('\n')).toUtf8();
        for (const QString &n : lists.at(i)) {
            if (!names.contains(n, Qt::CaseInsensitive)) {
                names << n;
            }
        }
    }
    QVariantList tagList;
    for (const QString &n : std::as_const(names)) {
        const QByteArray nb = n.toUtf8();
        tagList.append(QVariantMap{{QStringLiteral("name"), n},
                                   {QStringLiteral("text"), shown(n)},
                                   {QStringLiteral("colour"), TagLogic::colourFor(n)},
                                   {QStringLiteral("state"), int(telamon_tags_have(PropsBridge::p(packed), PropsBridge::n(packed), PropsBridge::p(nb), PropsBridge::n(nb)))}});
    }
    t[QStringLiteral("available")] = why.isEmpty();
    t[QStringLiteral("why")] = why;
    t[QStringLiteral("tags")] = tagList;
    t[QStringLiteral("hasTags")] = !names.isEmpty();
    m_tagsInfo = t;
    m_ratingAvailable = why.isEmpty() && allLocal;
    int rating = items.first()->rating;
    for (const Item *it : items) {
        if (it->rating != rating) {
            rating = -1;
            break;
        }
    }
    m_rating = m_ratingAvailable ? rating : -1;
    TagLogic::noteSeen(names);
    Q_EMIT attributesChanged();
}

void PropertiesLogic::startDetails()
{
    // One file on this computer: what KFileMetaData knows, read on a worker.
    if (m_items.size() != 1 || !m_items.first().ok || !m_items.first().local || m_items.first().isDir) {
        m_detailsLoaded = true;
        Q_EMIT detailsChanged();
        return;
    }
    const QString path = m_items.first().url.toLocalFile();
    const quint64 gen = m_gen;
    QPointer<PropertiesLogic> self(this);
    QThreadPool::globalInstance()->start([self, gen, path] {
        const MetaReader::Info info = MetaReader::read(path);
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, gen, info] {
            if (!self || gen != self->m_gen) {
                return;
            }
            self->m_details = info.rows;
            self->m_detailsLoaded = true;
            Q_EMIT self->detailsChanged();
        });
    });
}

QString PropertiesLogic::editableName() const
{
    return m_items.size() == 1 ? m_items.first().name : QString();
}

QUrl PropertiesLogic::renamedUrl(const QString &name) const
{
    if (m_items.size() != 1) {
        return {};
    }
    QUrl target = m_items.first().url.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    target.setPath(QDir::cleanPath(target.path() + QLatin1Char('/') + name));
    return target;
}

// ---- Default application ----

void PropertiesLogic::setDefaultApp(const QString &serviceId)
{
    const QString mime = m_openWith.value(QStringLiteral("mime")).toString();
    if (mime.isEmpty() || serviceId.isEmpty()) {
        return;
    }
    const KService::Ptr service = KService::serviceByStorageId(serviceId);
    if (!service) {
        return;
    }
    // The XDG file the desktop reads: the chosen application first.
    KConfig config(QStringLiteral("mimeapps.list"), KConfig::NoGlobals, QStandardPaths::GenericConfigLocation);
    for (const char *group : {"Default Applications", "Added Associations"}) {
        KConfigGroup g(&config, QLatin1String(group));
        QStringList list = g.readXdgListEntry(mime);
        list.removeAll(serviceId);
        list.prepend(serviceId);
        g.writeXdgListEntry(mime, list);
    }
    config.sync();
    QVariantMap ow = m_openWith;
    ow[QStringLiteral("current")] = QVariantMap{{QStringLiteral("id"), serviceId}, {QStringLiteral("name"), shown(service->name())}, {QStringLiteral("icon"), service->icon()}};
    m_openWith = ow;
    Q_EMIT openWithChanged();
}

// ---- Checksum ----

void PropertiesLogic::applyChecksum()
{
    QVariantList results;
    for (auto it = m_sums.cbegin(); it != m_sums.cend(); ++it) {
        const QString name = QString::fromUtf8(PropsBridge::bytesOf([&](uint8_t *o, size_t c) { return telamon_sum_name(uint32_t(it.key()), o, c); }));
        results.append(QVariantMap{{QStringLiteral("alg"), it.key()}, {QStringLiteral("name"), name}, {QStringLiteral("hex"), it.value()}});
    }
    m_checksum[QStringLiteral("results")] = results;
    m_checksum[QStringLiteral("running")] = m_sumRunning;
    m_checksum[QStringLiteral("progress")] = m_sumProgress;
    m_checksum[QStringLiteral("alg")] = m_sumAlg;
    m_checksum[QStringLiteral("error")] = m_sumError;
    Q_EMIT checksumChanged();
}

void PropertiesLogic::setChecksumState(bool running, double progress)
{
    m_sumRunning = running;
    m_sumProgress = progress;
    applyChecksum();
}

void PropertiesLogic::startChecksum(int alg)
{
    if (m_sumRunning || m_items.size() != 1 || !m_items.first().ok || !m_items.first().local || m_items.first().isDir || alg < 0 || alg > 3) {
        return;
    }
    m_sumError.clear();
    m_sumAlg = alg;
    m_sumCancel = std::make_shared<std::atomic<bool>>(false);
    const QString path = m_items.first().url.toLocalFile();
    const quint64 total = m_items.first().size;
    const quint64 gen = m_gen;
    setChecksumState(true, 0);
    QPointer<PropertiesLogic> self(this);
    auto cancel = m_sumCancel;
    QThreadPool::globalInstance()->start([self, gen, path, total, alg, cancel] {
        SumCtx ctx{self, gen, total, {}, -1000};
        ctx.since.start();
        const QByteArray p = QFile::encodeName(path);
        auto progress = [](void *user, uint64_t bytes) {
            auto *c = static_cast<SumCtx *>(user);
            // Tell the window about five times a second.
            if (c->since.elapsed() - c->last < 200 || !c->logic) {
                return;
            }
            c->last = c->since.elapsed();
            const double done = c->total > 0 ? qMin(1.0, double(bytes) / double(c->total)) : 0.0;
            QPointer<PropertiesLogic> logic = c->logic;
            const quint64 gen = c->gen;
            QMetaObject::invokeMethod(logic.data(), [logic, gen, done] {
                if (logic && gen == logic->m_gen && logic->m_sumRunning) {
                    logic->m_sumProgress = done;
                    logic->applyChecksum();
                }
            });
        };
        size_t len = 0;
        QByteArray out(256, 0);
        auto call = [&] {
            return telamon_sum_file(PropsBridge::p(p), PropsBridge::n(p), uint32_t(alg), reinterpret_cast<const uint8_t *>(cancel.get()), progress, &ctx,
                                    reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()), &len);
        };
        int rc = call();
        if (rc == 2 && len > size_t(out.size())) {
            out.resize(qsizetype(len));
            rc = call();
        }
        const QString text = QString::fromUtf8(out.constData(), qsizetype(qMin(len, size_t(out.size()))));
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, gen, text, alg, rc] {
            if (self && gen == self->m_gen) {
                self->checksumFinished(text, alg, rc, rc == 2 ? text : QString());
            }
        });
    });
}

void PropertiesLogic::checksumFinished(const QString &hex, int alg, int rc, const QString &error)
{
    m_sumRunning = false;
    m_sumProgress = rc == 0 ? 1.0 : 0.0;
    if (rc == 0) {
        m_sums.insert(alg, hex);
    } else if (rc == 2) {
        m_sumError = error;
        m_pendingCompareAlg = -1;
    }
    applyChecksum();
    updateCompare();
}

void PropertiesLogic::cancelChecksum()
{
    if (m_sumRunning && m_sumCancel) {
        *m_sumCancel = true;
        // The worker ends within a chunk; the window is told at once.
        m_pendingCompareAlg = -1;
    }
}

void PropertiesLogic::compareWith(const QString &text)
{
    m_expected = text;
    updateCompare();
}

void PropertiesLogic::updateCompare()
{
    QVariantMap c{{QStringLiteral("state"), QString()}, {QStringLiteral("text"), QString()}};
    if (!m_expected.trimmed().isEmpty()) {
        const QByteArray t = m_expected.toUtf8();
        uint32_t alg = 0;
        const QString hex = QString::fromUtf8(PropsBridge::bytesOf([&](uint8_t *o, size_t cap) { return telamon_sum_expected(PropsBridge::p(t), PropsBridge::n(t), &alg, o, cap); }));
        if (hex.isEmpty()) {
            c = {{QStringLiteral("state"), QStringLiteral("invalid")}, {QStringLiteral("text"), tr("That doesn't look like a checksum.")}};
        } else {
            const QString name = QString::fromUtf8(PropsBridge::bytesOf([&](uint8_t *o, size_t cap) { return telamon_sum_name(alg, o, cap); }));
            if (m_sums.contains(int(alg))) {
                const bool same = m_sums.value(int(alg)).compare(hex, Qt::CaseInsensitive) == 0;
                c = {{QStringLiteral("state"), same ? QStringLiteral("match") : QStringLiteral("mismatch")},
                     {QStringLiteral("text"), same ? tr("The %1 checksums match.").arg(name) : tr("The %1 checksums are different. The file is not the one that checksum is for.").arg(name)}};
            } else if (m_sumRunning) {
                c = {{QStringLiteral("state"), QStringLiteral("waiting")}, {QStringLiteral("text"), tr("Calculating, then comparing…")}};
            } else if (m_checksum.value(QStringLiteral("available")).toBool() && m_sumError.isEmpty()) {
                // A checksum of a kind not calculated yet: calculate it now.
                c = {{QStringLiteral("state"), QStringLiteral("waiting")}, {QStringLiteral("text"), tr("Calculating %1 to compare…").arg(name)}};
                m_pendingCompareAlg = int(alg);
                m_checksum[QStringLiteral("compare")] = c;
                startChecksum(int(alg));
                return;
            } else {
                c = {{QStringLiteral("state"), QStringLiteral("waiting")}, {QStringLiteral("text"), tr("No %1 checksum has been calculated.").arg(name)}};
            }
        }
    }
    m_checksum[QStringLiteral("compare")] = c;
    Q_EMIT checksumChanged();
}

// ---- Folder size ----

void PropertiesLogic::setFolderSizeState(const QString &state, const QString &text, bool running)
{
    m_sizeRunning = running;
    m_folderSize[QStringLiteral("state")] = state;
    m_folderSize[QStringLiteral("text")] = text;
    m_folderSize[QStringLiteral("running")] = running;
    Q_EMIT folderSizeChanged();
}

void PropertiesLogic::startFolderSize()
{
    if (m_sizeRunning) {
        return;
    }
    QList<QUrl> folders;
    bool allLocal = true;
    for (const Item &it : std::as_const(m_items)) {
        if (it.ok && it.isDir) {
            folders << it.url;
            allLocal = allLocal && it.local;
        }
    }
    if (folders.isEmpty()) {
        return;
    }
    m_sizeKnown = false;
    const quint64 gen = m_gen;
    setFolderSizeState(QStringLiteral("running"), tr("Counting…"), true);
    QPointer<PropertiesLogic> self(this);
    if (allLocal) {
        m_sizeCancel = std::make_shared<std::atomic<bool>>(false);
        auto cancel = m_sizeCancel;
        QStringList paths;
        for (const QUrl &u : folders) {
            paths << u.toLocalFile();
        }
        QThreadPool::globalInstance()->start([self, gen, paths, cancel] {
            struct Ctx {
                QPointer<PropertiesLogic> logic;
                quint64 gen;
                TelamonTotals before{};
                QElapsedTimer since;
                qint64 last = -1000;
            } ctx{self, gen, {}, {}, -1000};
            ctx.since.start();
            TelamonTotals sum{};
            bool stopped = false;
            for (const QString &path : paths) {
                const QByteArray p = QFile::encodeName(path);
                TelamonTotals t{};
                ctx.before = sum;
                const int rc = telamon_foldersize(
                    PropsBridge::p(p), PropsBridge::n(p), reinterpret_cast<const uint8_t *>(cancel.get()),
                    [](void *user, const TelamonTotals *now) {
                        auto *c = static_cast<Ctx *>(user);
                        if (c->since.elapsed() - c->last < 250 || !c->logic) {
                            return;
                        }
                        c->last = c->since.elapsed();
                        const quint64 files = c->before.files + now->files, bytes = c->before.bytes + now->bytes;
                        QPointer<PropertiesLogic> logic = c->logic;
                        const quint64 g = c->gen;
                        QMetaObject::invokeMethod(logic.data(), [logic, g, files, bytes] {
                            if (logic && g == logic->m_gen && logic->m_sizeRunning) {
                                logic->setFolderSizeState(QStringLiteral("running"), tr("Counting… %1 in %2 files so far").arg(KIO::convertSize(KIO::filesize_t(bytes)), QLocale().toString(files)), true);
                            }
                        });
                    },
                    &ctx, &t);
                sum.files += t.files;
                sum.folders += t.folders;
                sum.bytes += t.bytes;
                sum.on_disk += t.on_disk;
                sum.unreadable += t.unreadable;
                sum.skipped += t.skipped;
                if (rc == 1) {
                    stopped = true;
                    break;
                }
            }
            if (!self) {
                return;
            }
            QMetaObject::invokeMethod(self.data(), [self, gen, sum, stopped] {
                if (!self || gen != self->m_gen) {
                    return;
                }
                if (stopped) {
                    self->setFolderSizeState(QStringLiteral("stopped"), tr("Stopped. Counted so far: %1.").arg(KIO::convertSize(KIO::filesize_t(sum.bytes))), false);
                    return;
                }
                self->m_sizeKnown = true;
                self->m_sizeBytes = sum.bytes;
                self->m_sizeFiles = sum.files;
                self->m_sizeFolders = sum.folders;
                self->m_sizeOnDisk = sum.on_disk;
                QString text = tr("%1 in %2 and %3").arg(KIO::convertSize(KIO::filesize_t(sum.bytes)), counted(qint64(sum.files), "file", "files"), counted(qint64(sum.folders), "folder", "folders"));
                if (sum.unreadable > 0) {
                    text += QLatin1Char(' ') + tr("Some folders couldn't be read, so the total may be too small.");
                }
                if (sum.skipped > 0) {
                    text += QLatin1Char(' ') + tr("Folders on other drives inside it are not counted.");
                }
                self->setFolderSizeState(QStringLiteral("done"), text, false);
                self->assemble(false);
            });
        });
        return;
    }
    // Folders on a server: KIO counts them (it can be stopped, not measured).
    KFileItemList list;
    for (const QUrl &u : folders) {
        list << KFileItem(u, QStringLiteral("inode/directory"), S_IFDIR);
    }
    KIO::DirectorySizeJob *job = KIO::directorySize(list);
    m_sizeJob = job;
    connect(job, &KJob::result, this, [this, gen, job] {
        if (gen != m_gen) {
            return;
        }
        if (job->error()) {
            setFolderSizeState(job->error() == KJob::KilledJobError ? QStringLiteral("stopped") : QStringLiteral("error"), job->error() == KJob::KilledJobError ? tr("Stopped.") : tr("The size couldn't be counted."), false);
            return;
        }
        m_sizeKnown = true;
        m_sizeBytes = quint64(job->totalSize());
        m_sizeFiles = quint64(job->totalFiles());
        m_sizeFolders = quint64(job->totalSubdirs());
        m_sizeOnDisk = m_sizeBytes;
        setFolderSizeState(QStringLiteral("done"), tr("%1 in %2 and %3").arg(KIO::convertSize(KIO::filesize_t(m_sizeBytes)), counted(qint64(m_sizeFiles), "file", "files"), counted(qint64(m_sizeFolders), "folder", "folders")), false);
        assemble(false);
    });
}

void PropertiesLogic::cancelFolderSize()
{
    if (!m_sizeRunning) {
        return;
    }
    if (m_sizeCancel) {
        *m_sizeCancel = true;
    }
    if (m_sizeJob) {
        m_sizeJob->kill(KJob::EmitResult);
    }
}
