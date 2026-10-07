#include "FolderModel.h"

#include "RustBridge.h"

#include <KIO/Global>
#include <KIO/Job>
#include <KIO/UDSEntry>

#include <QFileInfo>
#include <QLocale>

#include <algorithm>

namespace
{
QString errorMessage(KIO::Job *job)
{
    // Plain words, never the job's own text: it can hold file names.
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

FolderModel::FolderModel(QObject *parent)
    : QAbstractListModel(parent)
    , m_lister(new KCoreDirLister(this))
{
    m_pool.setMaxThreadCount(1);
    m_lister->setDelayedMimeTypes(true);
    m_lister->setAutoErrorHandlingEnabled(false);
    m_sortTimer.setSingleShot(true);
    m_sortTimer.setInterval(60);
    connect(&m_sortTimer, &QTimer::timeout, this, &FolderModel::startSort);

    connect(m_lister, &KCoreDirLister::itemsAdded, this, [this](const QUrl &, const KFileItemList &items) { addItems(items); });
    connect(m_lister, &KCoreDirLister::itemsDeleted, this, &FolderModel::removeItems);
    connect(m_lister, &KCoreDirLister::refreshItems, this, &FolderModel::refreshItems);
    connect(m_lister, &KCoreDirLister::clear, this, &FolderModel::resetRows);
    connect(m_lister, &KCoreDirLister::completed, this, &FolderModel::onCompleted);
    connect(m_lister, &KCoreDirLister::canceled, this, [this] { setLoading(false); });
    connect(m_lister, &KCoreDirLister::jobError, this, &FolderModel::onJobError);
    connect(m_lister, &KCoreDirLister::redirection, this, [this](const QUrl &, const QUrl &to) {
        m_url = to;
        Q_EMIT urlChanged();
    });
}

FolderModel::~FolderModel()
{
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
        {ThumbnailSourceRole, "thumbnailSource"},
    };
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
    if (url == m_url && m_error.isEmpty()) {
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
        m_rows[i] = makeEntry(*found);
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
    m_listedUrl = m_url;
    const KFileItem root = m_lister->rootItem();
    const bool w = !root.isNull() && root.isWritable();
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
    setError(errorMessage(job));
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
    m_lister->emitChanges();
    Q_EMIT showHiddenChanged();
}

void FolderModel::setSortColumn(SortColumn c)
{
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
    auto rows = std::make_shared<QList<SortRowIn>>();
    rows->reserve(m_rows.size());
    for (Entry &e : m_rows) {
        if (byType) {
            fillType(e);
        }
        const bool needKey = e.key.isEmpty();
        rows->append({needKey || e.display.isEmpty() ? e.item.name() : QString(), e.key, byType ? e.type.toUtf8() : QByteArray(), e.size, e.mtime, e.ctime,
                      e.atime, e.isDir, e.display.isEmpty()});
    }
    const quint64 gen = m_structGen;
    const quint32 column = quint32(m_sortColumn);
    const bool desc = m_descending;
    const bool ff = m_foldersFirst;
    m_sortRunning = true;
    m_pool.start([this, rows, gen, column, desc, ff] {
        SortResult res;
        res.gen = gen;
        res.n = rows->size();
        res.keys.resize(res.n);
        res.displays.resize(res.n);
        std::vector<TelamonSortRow> flat(size_t(res.n));
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
            flat[size_t(i)] = {reinterpret_cast<const uint8_t *>(r.key.constData()),
                               size_t(r.key.size()),
                               reinterpret_cast<const uint8_t *>(r.kind.constData()),
                               size_t(r.kind.size()),
                               r.size,
                               r.mtime,
                               r.ctime,
                               r.atime,
                               r.isDir};
        }
        res.perm.resize(res.n);
        res.ok = telamon_sort_permutation(flat.data(), flat.size(), column, desc, ff, res.perm.data());
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
        QModelIndexList updated;
        updated.reserve(old.size());
        for (const QModelIndex &o : old) {
            updated.append(index(inverse[o.row()], o.column()));
        }
        changePersistentIndexList(old, updated);
        Q_EMIT layoutChanged({}, QAbstractItemModel::VerticalSortHint);
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
    return QItemSelection(index(std::min(from, to)), index(std::max(from, to)));
}

KFileItem FolderModel::fileItemOf(const QUrl &url) const
{
    const int row = rowOfUrl(url);
    return row >= 0 ? m_rows.at(row).item : KFileItem();
}
