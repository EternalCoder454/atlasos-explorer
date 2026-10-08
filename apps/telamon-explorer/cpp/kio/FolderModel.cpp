#include "FolderModel.h"

#include "RustBridge.h"

#include <KIO/Global>
#include <KIO/Job>
#include <KIO/UDSEntry>

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QLocale>
#include <QSet>

#include <sys/stat.h>

#include <cerrno>

#include <algorithm>
#include <numeric>

namespace
{
QString errorMessage(KIO::Job *job, bool archive)
{
    // Plain words, never the job's own text: it can hold file names.
    if (archive && job->error() != KIO::ERR_DOES_NOT_EXIST && job->error() != KIO::ERR_ACCESS_DENIED) {
        return FolderModel::tr("This archive couldn't be read. It may be damaged, or it may need a password, which Files can't enter.");
    }
    switch (job->error()) {
    case KIO::ERR_ACCESS_DENIED:
    case KIO::ERR_CANNOT_ENTER_DIRECTORY:
    case KIO::ERR_CANNOT_OPEN_FOR_READING:
        return FolderModel::tr("You don't have permission to open this folder.");
    case KIO::ERR_DOES_NOT_EXIST:
        return FolderModel::tr("This folder doesn't exist.");
    case KIO::ERR_WORKER_DIED:
    case KIO::ERR_CANNOT_CONNECT:
    case KIO::ERR_UNKNOWN_HOST:
    case KIO::ERR_SERVER_TIMEOUT:
        return FolderModel::tr("Couldn't reach the server.");
    case KIO::ERR_UNSUPPORTED_PROTOCOL:
        return FolderModel::tr("This kind of location isn't supported.");
    default:
        return FolderModel::tr("This folder couldn't be read.");
    }
}
}

QSet<QString> FolderModel::s_cut;
QList<FolderModel *> FolderModel::s_models;

FolderModel::FolderModel(QObject *parent)
    : QAbstractListModel(parent)
    , m_lister(new KCoreDirLister(this))
{
    s_models.append(this);
    connect(this, &FolderModel::urlChanged, this, &FolderModel::archiveChanged);
    connect(this, &FolderModel::searchingChanged, this, &FolderModel::archiveChanged);
    m_pool.setMaxThreadCount(1);
    m_lister->setDelayedMimeTypes(true);
    m_lister->setAutoErrorHandlingEnabled(false);
    m_sortTimer.setSingleShot(true);
    m_sortTimer.setInterval(60);
    connect(&m_sortTimer, &QTimer::timeout, this, &FolderModel::startSort);
    m_hiddenTimer.setSingleShot(true);
    m_hiddenTimer.setInterval(100);
    connect(&m_hiddenTimer, &QTimer::timeout, this, &FolderModel::recountHidden);

    // While the rows are search results the lister is stopped and its signals
    // change nothing; the folder is listed again when the search ends.
    connect(m_lister, &KCoreDirLister::itemsAdded, this, [this](const QUrl &, const KFileItemList &items) {
        if (!m_searching) {
            addItems(items);
        }
    });
    connect(m_lister, &KCoreDirLister::itemsDeleted, this, [this](const KFileItemList &items) {
        if (!m_searching) {
            removeItems(items);
        }
    });
    connect(m_lister, &KCoreDirLister::refreshItems, this, [this](const QList<QPair<KFileItem, KFileItem>> &items) {
        if (!m_searching) {
            refreshItems(items);
        }
    });
    connect(m_lister, &KCoreDirLister::clear, this, [this] {
        if (!m_searching) {
            resetRows();
        }
    });
    connect(m_lister, &KCoreDirLister::completed, this, [this] {
        if (!m_searching) {
            onCompleted();
        }
    });
    connect(m_lister, &KCoreDirLister::canceled, this, [this] {
        if (!m_searching) {
            setLoading(false);
        }
    });
    connect(m_lister, &KCoreDirLister::jobError, this, [this](KIO::Job *job) {
        if (!m_searching) {
            onJobError(job);
        }
    });
    connect(m_lister, &KCoreDirLister::redirection, this, [this](const QUrl &, const QUrl &to) {
        if (m_searching) {
            return;
        }
        m_url = to;
        Q_EMIT urlChanged();
    });
}

FolderModel::~FolderModel()
{
    s_models.removeAll(this);
    m_lister->disconnect(this);
    m_pool.clear();
    m_pool.waitForDone();
}

QHash<int, QByteArray> FolderModel::roleNames() const
{
    return {
        {Qt::DisplayRole, "display"},
        {NameRole, "name"},
        {UrlRole, "url"},
        {IconNameRole, "iconName"},
        {IsDirRole, "isDir"},
        {IsLinkRole, "isLink"},
        {IsHiddenRole, "isHidden"},
        {SizeRole, "size"},
        {ModifiedRole, "modified"},
        {SizeTextRole, "sizeText"},
        {ModifiedTextRole, "modifiedText"},
        {TypeTextRole, "typeText"},
        {PathTextRole, "pathText"},
        {ThumbnailSourceRole, "thumbnailSource"},
        {IsCutRole, "isCut"},
        {GroupRole, "groupKey"},
        {GroupCollapsedRole, "groupCollapsed"},
    };
}

bool FolderModel::inArchive() const
{
    return !m_searching && rustArchiveScheme(m_url.scheme());
}

bool FolderModel::isCut(const KFileItem &item) const
{
    return !s_cut.isEmpty() && s_cut.contains(item.url().adjusted(QUrl::StripTrailingSlash).toString(QUrl::FullyEncoded));
}

// Rows whose "waiting to be moved" changed are drawn again.
void FolderModel::setCutKeys(const QSet<QString> &keys)
{
    if (keys == s_cut) {
        return;
    }
    const QSet<QString> changed = (keys - s_cut) + (s_cut - keys);
    s_cut = keys;
    for (FolderModel *m : std::as_const(s_models)) {
        for (int i = 0; i < m->m_rows.size(); ++i) {
            const QString k = m->m_rows.at(i).item.url().adjusted(QUrl::StripTrailingSlash).toString(QUrl::FullyEncoded);
            if (changed.contains(k)) {
                Q_EMIT m->dataChanged(m->index(i), m->index(i), {IsCutRole});
            }
        }
    }
}

int FolderModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_rows.size());
}

FolderModel::Entry FolderModel::makeEntry(const KFileItem &item)
{
    Entry e;
    e.item = item;
    const KIO::UDSEntry u = item.entry();
    e.isDir = item.isDir();
    e.size = e.isDir ? 0 : quint64(item.size());
    e.mtime = u.numberValue(KIO::UDSEntry::UDS_MODIFICATION_TIME, 0);
    e.ctime = u.numberValue(KIO::UDSEntry::UDS_CREATION_TIME, 0);
    e.atime = u.numberValue(KIO::UDSEntry::UDS_ACCESS_TIME, 0);
    return e;
}

// By name only (MatchExtension): never a content check on the GUI thread.
void FolderModel::fillType(Entry &e) const
{
    if (!e.type.isEmpty()) {
        return;
    }
    if (e.isDir) {
        e.type = tr("Folder");
        e.icon = QStringLiteral("folder");
        return;
    }
    const QMimeType mt = m_mime.mimeTypeForFile(e.item.name(), QMimeDatabase::MatchExtension);
    e.type = mt.isDefault() ? tr("File") : mt.comment();
    e.icon = mt.iconName().isEmpty() ? QStringLiteral("application-octet-stream") : mt.iconName();
}

QVariant FolderModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_rows.size()) {
        return {};
    }
    Entry &e = m_rows[index.row()];
    switch (role) {
    case Qt::DisplayRole:
    case NameRole:
        if (e.display.isEmpty()) {
            e.display = rustDisplayName(e.item.name().toUtf8());
        }
        return e.display;
    case UrlRole:
        return e.item.url();
    case IconNameRole:
        fillType(e);
        return e.icon;
    case IsDirRole:
        return e.isDir;
    case IsLinkRole:
        return e.item.isLink();
    case IsHiddenRole:
        return e.item.isHidden();
    case IsCutRole:
        return isCut(e.item);
    case GroupRole:
        return grouped() ? e.group : QString();
    case GroupCollapsedRole:
        return grouped() && !m_collapsed.isEmpty() && m_collapsed.contains(e.group);
    case SizeRole:
        return e.size;
    case ModifiedRole:
        return e.mtime > 0 ? QVariant(QDateTime::fromSecsSinceEpoch(e.mtime)) : QVariant();
    case SizeTextRole:
        // Folders show nothing: their size is the directory entry's, not a count.
        return e.isDir ? QString() : KIO::convertSize(KIO::filesize_t(e.size));
    case ModifiedTextRole:
        return e.mtime > 0 ? QLocale().toString(QDateTime::fromSecsSinceEpoch(e.mtime), QLocale::ShortFormat) : QString();
    case TypeTextRole:
        fillType(e);
        return e.type;
    case PathTextRole:
        if (!m_searching) {
            return QString();
        }
        if (e.path.isNull()) {
            const QUrl parent = e.item.url().adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash);
            e.path = rustSearchPathText(parent.toString(QUrl::FullyEncoded | QUrl::RemovePassword), QDir::homePath());
        }
        return e.path;
    case ThumbnailSourceRole: {
        const QUrl u = e.item.url();
        if (e.isDir || !u.isLocalFile()) {
            return QString();
        }
        return QStringLiteral("image://thumb/") + QString::fromLatin1(u.toEncoded().toBase64(QByteArray::Base64UrlEncoding | QByteArray::OmitTrailingEquals))
            + QLatin1Char('/') + QString::number(e.mtime);
    }
    default:
        return {};
    }
}

void FolderModel::setUrl(const QUrl &url)
{
    if (m_searching) {
        // Going to a folder ends the search, even to the folder it was in.
        leaveSearch();
    } else if (url == m_url && m_error.isEmpty()) {
        return;
    }
    open(url, QString());
}

void FolderModel::open(const QUrl &url, const QString &notice)
{
    m_gone = false;
    ++m_structGen;
    m_sortDirty = false;
    m_sortTimer.stop();
    const bool urlDiffers = url != m_url;
    m_url = url;
    if (urlDiffers && !m_collapsed.isEmpty()) {
        m_collapsed.clear();
    }
    resetRows();
    if (m_notice != notice) {
        m_notice = notice;
        Q_EMIT noticeChanged();
    }
    setError(QString());
    if (m_canWrite) {
        m_canWrite = false;
        Q_EMIT canWriteChanged();
    }
    if (urlDiffers) {
        Q_EMIT urlChanged();
    }
    if (!url.isValid() || url.isEmpty()) {
        setLoading(false);
        setError(tr("This location can't be shown."));
        return;
    }
    setLoading(true);
    m_lister->openUrl(url, KCoreDirLister::NoFlags);
}

void FolderModel::refresh()
{
    if (m_searching) {
        Q_EMIT searchRefreshRequested();
        return;
    }
    if (!m_url.isValid()) {
        return;
    }
    setError(QString());
    setLoading(true);
    m_lister->openUrl(m_url, KCoreDirLister::Reload);
}

void FolderModel::resetRows()
{
    if (m_rows.isEmpty()) {
        return;
    }
    ++m_structGen;
    beginResetModel();
    m_rows.clear();
    m_folders = 0;
    endResetModel();
    Q_EMIT countChanged();
    m_hiddenTimer.start();
}

void FolderModel::setLoading(bool on)
{
    if (m_loading != on) {
        m_loading = on;
        Q_EMIT loadingChanged();
    }
}

void FolderModel::setError(const QString &text)
{
    if (m_error != text) {
        m_error = text;
        Q_EMIT errorTextChanged();
    }
}

void FolderModel::updateCounts()
{
    Q_EMIT countChanged();
    m_hiddenTimer.start();
}

// The lister keeps the hidden items it doesn't show: what it holds beyond the
// rows is what is hidden.
void FolderModel::recountHidden()
{
    const int hidden = m_showHidden || m_searching ? 0 : qMax(0, int(m_lister->items(KCoreDirLister::AllItems).size()) - int(m_rows.size()));
    if (hidden != m_hidden) {
        m_hidden = hidden;
        Q_EMIT hiddenCountChanged();
    }
}

void FolderModel::addItems(const KFileItemList &items)
{
    if (items.isEmpty()) {
        return;
    }
    QList<Entry> batch;
    batch.reserve(items.size());
    int dirs = 0;
    for (const KFileItem &it : items) {
        batch.append(makeEntry(it));
        dirs += batch.last().isDir;
    }
    const int first = int(m_rows.size());
    beginInsertRows({}, first, first + int(batch.size()) - 1);
    m_rows.append(std::move(batch));
    m_folders += dirs;
    endInsertRows();
    updateCounts();
    scheduleSort();
}

void FolderModel::removeItems(const KFileItemList &items)
{
    QHash<QString, QUrl> names;
    for (const KFileItem &it : items) {
        if (it.url() == m_url && !m_gone) {
            folderGone(QString());
            return;
        }
        names.insert(it.name(), it.url());
    }
    // Rows to remove, found with one pass; removed in ranges from the end.
    QList<int> rows;
    for (int i = 0; i < m_rows.size(); ++i) {
        const auto found = names.constFind(m_rows[i].item.name());
        if (found != names.cend() && *found == m_rows[i].item.url()) {
            rows.append(i);
        }
    }
    if (rows.isEmpty()) {
        return;
    }
    ++m_structGen;
    for (int k = int(rows.size()) - 1; k >= 0;) {
        int hi = rows[k];
        int j = k;
        while (j > 0 && rows[j - 1] == rows[j] - 1) {
            --j;
        }
        const int lo = rows[j];
        beginRemoveRows({}, lo, hi);
        for (int r = hi; r >= lo; --r) {
            m_folders -= m_rows[r].isDir;
            m_rows.removeAt(r);
        }
        endRemoveRows();
        k = j - 1;
    }
    updateCounts();
    scheduleSort();
}

void FolderModel::refreshItems(const QList<QPair<KFileItem, KFileItem>> &items)
{
    QHash<QString, KFileItem> names;
    for (const auto &p : items) {
        names.insert(p.first.name(), p.second);
    }
    bool any = false;
    for (int i = 0; i < m_rows.size(); ++i) {
        const auto found = names.constFind(m_rows[i].item.name());
        if (found == names.cend()) {
            continue;
        }
        m_folders -= m_rows[i].isDir;
        const QString group = m_rows[i].group;
        m_rows[i] = makeEntry(*found);
        m_rows[i].group = group;
        m_folders += m_rows[i].isDir;
        Q_EMIT dataChanged(index(i), index(i));
        any = true;
    }
    if (any) {
        ++m_structGen;
        updateCounts();
        scheduleSort();
    }
}

void FolderModel::onCompleted()
{
    setLoading(false);
    m_hiddenTimer.start();
    m_listedUrl = m_url;
    const KFileItem root = m_lister->rootItem();
    // An archive is opened for reading only (kio-extras' worker reports its
    // folders as writable).
    const bool w = !root.isNull() && root.isWritable() && !inArchive();
    if (w != m_canWrite) {
        m_canWrite = w;
        Q_EMIT canWriteChanged();
    }
    if (m_sortDirty) {
        m_sortTimer.stop();
        startSort();
    }
}

void FolderModel::onJobError(KIO::Job *job)
{
    if (!job) {
        return;
    }
    const int code = job->error();
    setLoading(false);
    if (code == KIO::ERR_DOES_NOT_EXIST && m_url.isLocalFile() && !m_gone && m_listedUrl == m_url) {
        folderGone(QString());
        return;
    }
    setError(errorMessage(job, inArchive()));
}

// The folder is gone: the nearest parent that exists is shown instead.
void FolderModel::folderGone(const QString &)
{
    m_gone = true;
    const QUrl gone = m_url;
    const quint64 gen = ++m_structGen;
    m_pool.start([this, gone, gen] {
        QUrl up = gone;
        if (gone.isLocalFile()) {
            QString path = gone.toLocalFile();
            while (path.size() > 1 && !QFileInfo::exists(path)) {
                path = QFileInfo(path).absolutePath();
            }
            up = QUrl::fromLocalFile(path);
        } else {
            up = gone.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
        }
        QMetaObject::invokeMethod(this, [this, up, gen] {
            if (gen == m_structGen) {
                open(up, tr("The folder was removed, so its parent is shown."));
            }
        }, Qt::QueuedConnection);
    });
}

void FolderModel::setShowHidden(bool on)
{
    if (m_showHidden == on) {
        return;
    }
    m_showHidden = on;
    m_lister->setShowHiddenFiles(on);
    if (!m_searching) {
        m_lister->emitChanges();
    }
    Q_EMIT showHiddenChanged();
    m_hiddenTimer.start();
}

void FolderModel::setSortColumn(SortColumn c)
{
    // Relevance is the order of a search's results: it exists only then.
    if (c == Relevance && !m_searching) {
        return;
    }
    if (m_sortColumn != c) {
        m_sortColumn = c;
        ++m_structGen;
        scheduleSort();
        Q_EMIT sortChanged();
    }
}

void FolderModel::setSortDescending(bool on)
{
    if (m_descending != on) {
        m_descending = on;
        ++m_structGen;
        scheduleSort();
        Q_EMIT sortChanged();
    }
}

void FolderModel::setFoldersFirst(bool on)
{
    if (m_foldersFirst != on) {
        m_foldersFirst = on;
        ++m_structGen;
        scheduleSort();
        Q_EMIT sortChanged();
    }
}

void FolderModel::setGroupBy(GroupBy g)
{
    if (m_groupBy == g) {
        return;
    }
    m_groupBy = g;
    m_collapsed.clear();
    m_groupCounts.clear();
    ++m_structGen;
    // The old names must not be shown under the new grouping.
    for (Entry &e : m_rows) {
        e.group.clear();
    }
    if (!m_rows.isEmpty()) {
        Q_EMIT dataChanged(index(0), index(int(m_rows.size()) - 1), {GroupRole, GroupCollapsedRole});
    }
    scheduleSort();
    Q_EMIT groupChanged();
    Q_EMIT sortChanged();
}

void FolderModel::scheduleSort()
{
    m_sortDirty = true;
    if (!m_loading) {
        // A sort or a change after loading: at once.
        m_sortTimer.start(0);
    } else if (!m_sortTimer.isActive()) {
        m_sortTimer.start();
    }
}

void FolderModel::startSort()
{
    if (m_sortRunning || m_rows.isEmpty()) {
        if (m_rows.isEmpty()) {
            m_sortDirty = false;
        }
        return;
    }
    m_sortDirty = false;
    const bool byType = m_sortColumn == Type;
    const GroupBy group = grouped() && m_sortColumn != Relevance ? m_groupBy : GroupNone;
    const bool needKind = byType || group == GroupType;
    auto rows = std::make_shared<QList<SortRowIn>>();
    rows->reserve(m_rows.size());
    for (Entry &e : m_rows) {
        if (needKind) {
            fillType(e);
        }
        const bool needKey = e.key.isEmpty();
        rows->append({needKey || e.display.isEmpty() ? e.item.name() : QString(), e.key, needKind ? e.type.toUtf8() : QByteArray(), e.size, e.mtime, e.ctime,
                      e.atime, e.isDir, e.display.isEmpty(), e.rank});
    }
    const quint64 gen = m_structGen;
    const quint32 column = quint32(m_sortColumn);
    const bool desc = m_descending;
    const bool ff = m_foldersFirst;
    // The date groups are told apart from "now" in this zone, the weeks from the locale's first day.
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    const qint64 tz = QDateTime::currentDateTime().offsetFromUtc();
    const quint32 weekStart = quint32(QLocale().firstDayOfWeek()) - 1;
    const bool reverseGroups = group != GroupNone && telamon_group_reversed(quint32(group), column, desc);
    m_sortRunning = true;
    m_pool.start([this, rows, gen, column, desc, ff, group, now, tz, weekStart, reverseGroups] {
        SortResult res;
        res.gen = gen;
        res.n = rows->size();
        res.keys.resize(res.n);
        res.displays.resize(res.n);
        if (group != GroupNone) {
            res.groups.resize(res.n);
        }
        std::vector<TelamonSortRow> flat(size_t(res.n));
        // The order key of each row's group, kept alive until the sort is done.
        std::vector<QByteArray> orders(group != GroupNone ? size_t(res.n) : 0);
        QHash<QByteArray, QString> labels;
        for (qsizetype i = 0; i < res.n; ++i) {
            SortRowIn &r = (*rows)[i];
            if (!r.name.isEmpty()) {
                const QByteArray utf8 = r.name.toUtf8();
                if (r.key.isEmpty()) {
                    res.keys[i] = rustNameKey(utf8);
                    r.key = res.keys[i];
                }
                if (r.needDisplay) {
                    res.displays[i] = rustDisplayName(utf8);
                }
            }
            if (group != GroupNone) {
                QByteArray out(96, 0);
                auto call = [&] {
                    return telamon_group_of(quint32(group), reinterpret_cast<const uint8_t *>(r.key.constData()), size_t(r.key.size()),
                                            reinterpret_cast<const uint8_t *>(r.kind.constData()), size_t(r.kind.size()), r.isDir, r.mtime, now, tz,
                                            weekStart, reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
                };
                size_t n = call();
                if (n > size_t(out.size())) {
                    out.resize(qsizetype(n));
                    n = call();
                }
                out.truncate(qsizetype(n));
                const qsizetype zero = out.indexOf('\0');
                orders[size_t(i)] = zero < 0 ? out : out.left(zero);
                // One string per group, shared by its rows.
                auto found = labels.constFind(orders[size_t(i)]);
                if (found == labels.cend()) {
                    found = labels.insert(orders[size_t(i)], zero < 0 ? QString() : QString::fromUtf8(out.constData() + zero + 1, qsizetype(out.size() - zero - 1)));
                }
                res.groups[i] = *found;
            }
            flat[size_t(i)] = {reinterpret_cast<const uint8_t *>(r.key.constData()),
                               size_t(r.key.size()),
                               reinterpret_cast<const uint8_t *>(r.kind.constData()),
                               size_t(r.kind.size()),
                               group != GroupNone ? reinterpret_cast<const uint8_t *>(orders[size_t(i)].constData()) : nullptr,
                               group != GroupNone ? size_t(orders[size_t(i)].size()) : 0,
                               r.size,
                               r.mtime,
                               r.ctime,
                               r.atime,
                               r.isDir};
        }
        res.perm.resize(res.n);
        if (column == quint32(Relevance)) {
            // Search results: the order the search gave them in.
            std::iota(res.perm.begin(), res.perm.end(), quint32(0));
            std::stable_sort(res.perm.begin(), res.perm.end(), [&rows](quint32 a, quint32 b) { return (*rows)[a].rank < (*rows)[b].rank; });
            res.ok = true;
        } else {
            res.ok = telamon_sort_permutation(flat.data(), flat.size(), column, desc, ff, reverseGroups, res.perm.data());
        }
        QMetaObject::invokeMethod(this, [this, res = std::move(res)]() mutable { applySort(std::move(res)); }, Qt::QueuedConnection);
    });
}

void FolderModel::applySort(SortResult r)
{
    m_sortRunning = false;
    // Rows may have been appended since the snapshot; nothing else may have moved.
    if (r.ok && r.gen == m_structGen && r.n <= m_rows.size()) {
        for (qsizetype i = 0; i < r.n; ++i) {
            if (!r.keys[i].isEmpty()) {
                m_rows[i].key = r.keys[i];
            }
            if (!r.displays[i].isEmpty() && m_rows[i].display.isEmpty()) {
                m_rows[i].display = r.displays[i];
            }
            if (i < r.groups.size()) {
                m_rows[i].group = r.groups[i];
            }
        }
        Q_EMIT layoutAboutToBeChanged({}, QAbstractItemModel::VerticalSortHint);
        const QModelIndexList old = persistentIndexList();
        QList<int> inverse(m_rows.size());
        QList<Entry> sorted;
        sorted.reserve(m_rows.size());
        for (qsizetype i = 0; i < r.n; ++i) {
            inverse[int(r.perm[i])] = int(i);
            sorted.append(std::move(m_rows[int(r.perm[i])]));
        }
        for (qsizetype i = r.n; i < m_rows.size(); ++i) {
            inverse[int(i)] = int(i);
            sorted.append(std::move(m_rows[int(i)]));
        }
        m_rows = std::move(sorted);
        m_groupCounts.clear();
        if (!r.groups.isEmpty()) {
            for (const Entry &e : std::as_const(m_rows)) {
                if (!e.group.isEmpty()) {
                    ++m_groupCounts[e.group];
                }
            }
        }
        QModelIndexList updated;
        updated.reserve(old.size());
        for (const QModelIndex &o : old) {
            updated.append(index(inverse[o.row()], o.column()));
        }
        changePersistentIndexList(old, updated);
        Q_EMIT layoutChanged({}, QAbstractItemModel::VerticalSortHint);
        if (!r.groups.isEmpty() || m_groupRevision != 0) {
            ++m_groupRevision;
            Q_EMIT groupRevisionChanged();
        }
    }
    if (m_sortDirty || r.gen != m_structGen || r.n < m_rows.size()) {
        m_sortDirty = true;
        m_sortTimer.start(m_loading ? 60 : 0);
    }
}

QUrl FolderModel::urlAt(int row) const
{
    return row >= 0 && row < m_rows.size() ? m_rows[row].item.url() : QUrl();
}

bool FolderModel::isDirAt(int row) const
{
    return row >= 0 && row < m_rows.size() && m_rows[row].isDir;
}

int FolderModel::rowOfUrl(const QUrl &url) const
{
    for (int i = 0; i < m_rows.size(); ++i) {
        if (m_rows[i].item.url() == url) {
            return i;
        }
    }
    // A folder's URL may come with a slash at its end (its parent's listing
    // never has one).
    const QUrl bare = url.adjusted(QUrl::StripTrailingSlash);
    if (bare != url) {
        for (int i = 0; i < m_rows.size(); ++i) {
            if (m_rows[i].item.url() == bare) {
                return i;
            }
        }
    }
    return -1;
}

QVariantList FolderModel::urlsOf(const QVariantList &rows) const
{
    QVariantList out;
    out.reserve(rows.size());
    for (const QVariant &v : rows) {
        const int r = v.toInt();
        if (r >= 0 && r < m_rows.size()) {
            out.append(m_rows[r].item.url());
        }
    }
    return out;
}

int FolderModel::findPrefix(const QString &prefix, int startRow) const
{
    const int n = int(m_rows.size());
    if (n == 0 || prefix.isEmpty()) {
        return -1;
    }
    startRow = std::clamp(startRow, 0, n - 1);
    for (int k = 0; k < n; ++k) {
        Entry &e = m_rows[(startRow + k) % n];
        if (!m_collapsed.isEmpty() && grouped() && m_collapsed.contains(e.group)) {
            continue;
        }
        if (e.display.isEmpty()) {
            e.display = rustDisplayName(e.item.name().toUtf8());
        }
        if (e.display.startsWith(prefix, Qt::CaseInsensitive)) {
            return (startRow + k) % n;
        }
    }
    return -1;
}

QItemSelection FolderModel::rangeSelection(int from, int to) const
{
    const int n = int(m_rows.size());
    if (n == 0) {
        return {};
    }
    from = std::clamp(from, 0, n - 1);
    to = std::clamp(to, 0, n - 1);
    const int lo = std::min(from, to), hi = std::max(from, to);
    if (m_collapsed.isEmpty() || !grouped()) {
        return QItemSelection(index(lo), index(hi));
    }
    // The rows in collapsed groups are not there to be selected.
    QItemSelection out;
    int start = -1;
    for (int i = lo; i <= hi; ++i) {
        const bool hidden = m_collapsed.contains(m_rows.at(i).group);
        if (!hidden && start < 0) {
            start = i;
        } else if (hidden && start >= 0) {
            out.select(index(start), index(i - 1));
            start = -1;
        }
    }
    if (start >= 0) {
        out.select(index(start), index(hi));
    }
    return out;
}

QList<FolderModel::GroupSpan> FolderModel::groupSpans() const
{
    QList<GroupSpan> out;
    if (!grouped()) {
        return out;
    }
    for (int i = 0; i < m_rows.size(); ++i) {
        const QString &g = m_rows.at(i).group;
        if (out.isEmpty() || out.last().label != g) {
            out.append({g, i, 1});
        } else {
            ++out.last().count;
        }
    }
    return out;
}

void FolderModel::toggleGroup(const QString &group)
{
    if (!grouped() || group.isEmpty()) {
        return;
    }
    if (!m_collapsed.remove(group)) {
        m_collapsed.insert(group);
    }
    const QVariantList range = groupRange(group);
    if (range.size() == 2) {
        Q_EMIT dataChanged(index(range[0].toInt()), index(range[1].toInt()), {GroupCollapsedRole});
    }
    // The headers read their state again.
    ++m_groupRevision;
    Q_EMIT groupRevisionChanged();
}

QVariantList FolderModel::groupRange(const QString &group) const
{
    int first = -1, last = -1;
    for (int i = 0; i < m_rows.size(); ++i) {
        if (m_rows.at(i).group == group) {
            if (first < 0) {
                first = i;
            }
            last = i;
        }
    }
    return first < 0 ? QVariantList() : QVariantList{first, last};
}

bool FolderModel::isRowCollapsed(int row) const
{
    return row >= 0 && row < m_rows.size() && !m_collapsed.isEmpty() && grouped() && m_collapsed.contains(m_rows.at(row).group);
}

int FolderModel::visibleRowFrom(int row, int step) const
{
    while (row >= 0 && row < m_rows.size() && isRowCollapsed(row)) {
        row += step < 0 ? -1 : 1;
    }
    return row >= 0 && row < m_rows.size() ? row : -1;
}

QVariantMap FolderModel::detailsAt(int row) const
{
    if (row < 0 || row >= m_rows.size()) {
        return {};
    }
    const QModelIndex idx = index(row, 0);
    const Entry &e = m_rows.at(row);
    const auto when = [](qint64 secs) { return secs > 0 ? QLocale().toString(QDateTime::fromSecsSinceEpoch(secs), QLocale::ShortFormat) : QString(); };
    // Where the file is on this computer, if it is: a file: URL, or the real
    // place the Trash's worker lists its files with. No other worker's word
    // is taken for a path that is then read.
    QString local;
    const QUrl url = e.item.url();
    if (url.isLocalFile()) {
        local = url.toLocalFile();
    } else if (url.scheme() == QLatin1String("trash")) {
        local = e.item.localPath();
        if (!QDir::isAbsolutePath(local)) {
            local.clear();
        }
    }
    return {{QStringLiteral("name"), data(idx, NameRole)},
            {QStringLiteral("url"), url},
            {QStringLiteral("localPath"), local},
            {QStringLiteral("isDir"), e.isDir},
            {QStringLiteral("isLink"), e.item.isLink()},
            {QStringLiteral("size"), qlonglong(e.size)},
            {QStringLiteral("mtime"), qlonglong(e.mtime)},
            {QStringLiteral("typeText"), data(idx, TypeTextRole)},
            {QStringLiteral("iconName"), data(idx, IconNameRole)},
            {QStringLiteral("sizeText"), data(idx, SizeTextRole)},
            {QStringLiteral("modifiedText"), when(e.mtime)},
            {QStringLiteral("createdText"), when(e.ctime)},
            {QStringLiteral("pathText"), data(idx, PathTextRole)}};
}

QVariantList FolderModel::filterExisting(const QVariantList &urls) const
{
    QSet<QUrl> shown;
    shown.reserve(m_rows.size());
    for (const Entry &e : std::as_const(m_rows)) {
        shown.insert(e.item.url());
    }
    QVariantList out;
    for (const QVariant &v : urls) {
        if (shown.contains(v.toUrl())) {
            out.append(v);
        }
    }
    return out;
}

QVariantMap FolderModel::selectionStats(const QVariantList &rows) const
{
    int files = 0, folders = 0;
    double bytes = 0;
    for (const QVariant &v : rows) {
        const int r = v.toInt();
        if (r < 0 || r >= m_rows.size()) {
            continue;
        }
        const Entry &e = m_rows.at(r);
        if (e.isDir) {
            ++folders;
        } else {
            ++files;
            bytes += double(e.size);
        }
    }
    return {{QStringLiteral("files"), files}, {QStringLiteral("folders"), folders}, {QStringLiteral("bytes"), bytes}};
}

KFileItem FolderModel::fileItemOf(const QUrl &url) const
{
    const int row = rowOfUrl(url);
    return row >= 0 ? m_rows.at(row).item : KFileItem();
}

// ---- Search results ----

FolderModel::Entry FolderModel::makeSearchEntry(const SearchHit &hit, quint32 rank)
{
    KIO::UDSEntry u;
    // A file on this computer is named by its URL (the real name, which the
    // index's display name may have changed); the name given is for the rest.
    // KFileItem takes its name from the entry only, never from the URL, so
    // without UDS_NAME the row has no name at all.
    u.fastInsert(KIO::UDSEntry::UDS_NAME, hit.url.isLocalFile() ? hit.url.fileName() : hit.name);
    u.fastInsert(KIO::UDSEntry::UDS_FILE_TYPE, hit.isDir ? S_IFDIR : S_IFREG);
    u.fastInsert(KIO::UDSEntry::UDS_SIZE, qlonglong(hit.size));
    u.fastInsert(KIO::UDSEntry::UDS_MODIFICATION_TIME, qlonglong(hit.mtime));
    if (hit.url.isLocalFile()) {
        u.fastInsert(KIO::UDSEntry::UDS_LOCAL_PATH, hit.url.toLocalFile());
    }
    Entry e = makeEntry(KFileItem(u, hit.url));
    e.rank = rank;
    return e;
}

void FolderModel::beginSearch()
{
    if (m_searching) {
        return;
    }
    m_searching = true;
    m_gone = false;
    // The lister stops: its signals are ignored from here (see the constructor).
    m_lister->stop();
    ++m_structGen;
    m_sortDirty = false;
    m_sortTimer.stop();
    m_folderSortColumn = m_sortColumn;
    m_folderSortDescending = m_descending;
    m_sortColumn = Relevance;
    m_descending = false;
    resetRows();
    m_nextRank = 0;
    if (!m_notice.isEmpty()) {
        m_notice.clear();
        Q_EMIT noticeChanged();
    }
    setError(QString());
    setLoading(false);
    if (!m_canWrite) {
        // Results are files from many folders; each operation says if it fails.
        m_canWrite = true;
        Q_EMIT canWriteChanged();
    }
    if (m_hidden != 0) {
        m_hidden = 0;
        Q_EMIT hiddenCountChanged();
    }
    Q_EMIT sortChanged();
    Q_EMIT searchingChanged();
    Q_EMIT groupChanged();
}

// Back to the folder's own sort, without listing it.
void FolderModel::leaveSearch()
{
    m_searching = false;
    ++m_structGen;
    m_sortDirty = false;
    m_sortTimer.stop();
    m_sortColumn = m_folderSortColumn;
    m_descending = m_folderSortDescending;
    setLoading(false);
    Q_EMIT sortChanged();
    Q_EMIT searchingChanged();
    Q_EMIT groupChanged();
}

void FolderModel::endSearch()
{
    if (!m_searching) {
        return;
    }
    leaveSearch();
    // The folder, listed again (KIO has it cached, so this is quick).
    open(m_url, QString());
}

void FolderModel::setSearchResults(const QList<SearchHit> &hits)
{
    if (!m_searching) {
        return;
    }
    ++m_structGen;
    QList<Entry> rows;
    rows.reserve(hits.size());
    int dirs = 0;
    m_nextRank = 0;
    for (const SearchHit &h : hits) {
        rows.append(makeSearchEntry(h, m_nextRank++));
        dirs += rows.last().isDir;
    }
    beginResetModel();
    m_rows = std::move(rows);
    m_folders = dirs;
    endResetModel();
    Q_EMIT countChanged();
    if (m_sortColumn != Relevance) {
        scheduleSort();
    }
}

void FolderModel::appendSearchResults(const QList<SearchHit> &hits)
{
    if (!m_searching || hits.isEmpty()) {
        return;
    }
    QList<Entry> batch;
    batch.reserve(hits.size());
    int dirs = 0;
    for (const SearchHit &h : hits) {
        batch.append(makeSearchEntry(h, m_nextRank++));
        dirs += batch.last().isDir;
    }
    const int first = int(m_rows.size());
    beginInsertRows({}, first, first + int(batch.size()) - 1);
    m_rows.append(std::move(batch));
    m_folders += dirs;
    endInsertRows();
    Q_EMIT countChanged();
    if (m_sortColumn != Relevance) {
        scheduleSort();
    }
}

void FolderModel::pruneSearchResults()
{
    if (!m_searching || m_rows.isEmpty()) {
        return;
    }
    QList<QUrl> urls;
    urls.reserve(m_rows.size());
    for (const Entry &e : std::as_const(m_rows)) {
        if (e.item.url().isLocalFile()) {
            urls.append(e.item.url());
        }
    }
    const quint64 gen = m_structGen;
    m_pool.start([this, urls, gen] {
        QSet<QUrl> gone;
        for (const QUrl &u : urls) {
            struct stat st;
            if (::lstat(QFile::encodeName(u.toLocalFile()).constData(), &st) != 0 && errno == ENOENT) {
                gone.insert(u);
            }
        }
        if (gone.isEmpty()) {
            return;
        }
        QMetaObject::invokeMethod(
            this,
            [this, gone, gen] {
                // Results that were replaced meanwhile are checked by the next search.
                if (m_searching && gen == m_structGen) {
                    removeSearchRows(gone);
                }
            },
            Qt::QueuedConnection);
    });
}

void FolderModel::removeSearchRows(const QSet<QUrl> &gone)
{
    QList<int> rows;
    for (int i = 0; i < m_rows.size(); ++i) {
        if (gone.contains(m_rows[i].item.url())) {
            rows.append(i);
        }
    }
    if (rows.isEmpty()) {
        return;
    }
    for (int k = int(rows.size()) - 1; k >= 0;) {
        int hi = rows[k];
        int j = k;
        while (j > 0 && rows[j - 1] == rows[j] - 1) {
            --j;
        }
        const int lo = rows[j];
        beginRemoveRows({}, lo, hi);
        for (int r = hi; r >= lo; --r) {
            m_folders -= m_rows[r].isDir;
            m_rows.removeAt(r);
        }
        endRemoveRows();
        k = j - 1;
    }
    ++m_structGen;
    Q_EMIT countChanged();
}
