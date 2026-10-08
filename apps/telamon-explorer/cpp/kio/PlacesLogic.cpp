#include "PlacesLogic.h"
#include "RustBridge.h"

#include <KCoreDirLister>
#include <KFileItem>
#include <KFilePlacesModel>
#include <KIO/DirectorySizeJob>
#include <KIO/EmptyTrashJob>
#include <KIO/Global>
#include <KIO/Job>
#include <KIO/StatJob>

#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QStorageInfo>
#include <QThreadPool>

#include <solid/device.h>
#include <solid/portablemediaplayer.h>
#include <solid/storageaccess.h>

#include <QProcess>

#include <functional>
#include <memory>

namespace
{
// Actions, as the core numbers them (places::ACT_*).
enum Act : quint32 {
    ActRename = 1u << 0,
    ActHide = 1u << 1,
    ActUnhide = 1u << 2,
    ActRemove = 1u << 3,
    ActMount = 1u << 4,
    ActUnmount = 1u << 5,
    ActOpenInDisks = 1u << 6,
    ActEmptyTrash = 1u << 7,
    ActNewTab = 1u << 8,
    ActReorder = 1u << 9,
    ActAcceptsFiles = 1u << 10,
};

// How often the disks' usage is read again while a drive is mounted.
constexpr int UsageRefreshMs = 30000;
// A mount that never answers is forgotten after this long.
constexpr int PendingMs = 60000;
// The Trash's size is waited for this long before the question is asked.
constexpr int TrashSizeMs = 6000;
// A drop of more than this many items is taken for files to move, not folders to pin.
constexpr int MaxPinsPerDrop = 32;

QString clean(const QString &s)
{
    return rustDisplayName(s.toUtf8());
}

bool diskLocal(const QUrl &u)
{
    return u.isLocalFile();
}

// A URL as places compare them: no trailing slash, no "..".
QUrl normalized(const QUrl &u)
{
    QUrl n = u.adjusted(QUrl::NormalizePathSegments | QUrl::StripTrailingSlash | QUrl::RemovePassword);
    if (n.isLocalFile() && n.path().isEmpty()) {
        n.setPath(QStringLiteral("/"));
    }
    return n;
}
}

PlacesLogic *PlacesLogic::instance()
{
    // Owned by the application object: it outlives the QML engine, which
    // reads it until the engine is gone.
    static PlacesLogic *self = new PlacesLogic(qApp);
    return self;
}

PlacesLogic *PlacesLogic::create(QQmlEngine *, QJSEngine *)
{
    PlacesLogic *p = instance();
    QQmlEngine::setObjectOwnership(p, QQmlEngine::CppOwnership);
    return p;
}

PlacesLogic::PlacesLogic(QObject *parent)
    : QObject(parent)
{
    // The places file is created (with Home, Trash and Network) by the model
    // itself on first use. A file that is not there yet is how a first run is
    // told from a user whose list we leave alone.
    const bool firstRun = !QFile::exists(QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) + QStringLiteral("/user-places.xbel"));

    m_src = new KFilePlacesModel(this);
    if (firstRun) {
        seedStandardPlaces();
    }

    m_rebuildTimer.setSingleShot(true);
    m_rebuildTimer.setInterval(30);
    connect(&m_rebuildTimer, &QTimer::timeout, this, &PlacesLogic::rebuild);
    const auto changed = [this] { scheduleRebuild(); };
    connect(m_src, &QAbstractItemModel::rowsInserted, this, changed);
    connect(m_src, &QAbstractItemModel::rowsRemoved, this, changed);
    connect(m_src, &QAbstractItemModel::rowsMoved, this, changed);
    connect(m_src, &QAbstractItemModel::modelReset, this, changed);
    connect(m_src, &QAbstractItemModel::layoutChanged, this, changed);
    connect(m_src, &QAbstractItemModel::dataChanged, this, changed);
    connect(m_src, &KFilePlacesModel::reloaded, this, changed);

    // Setup and teardown report through here; a drive that answers only by
    // changing (the model's data changes) is handled by checkPending().
    connect(m_src, &KFilePlacesModel::setupDone, this, [this](const QModelIndex &index, bool success) {
        const auto it = m_pending.constFind(keyAt(index.row()));
        if (!success && it != m_pending.constEnd() && !it->unmount) {
            if (!quietAfterError()) {
                emit message(tr("Couldn't mount %1.").arg(it->name));
            }
            m_pending.erase(it);
        }
        rebuild();
    });
    connect(m_src, &KFilePlacesModel::teardownDone, this, [this](const QModelIndex &index, Solid::ErrorType error, const QVariant &data) {
        const auto it = m_pending.constFind(keyAt(index.row()));
        if (error != Solid::NoError && it != m_pending.constEnd() && it->unmount) {
            if (!quietAfterError()) {
                const QString why = clean(data.toString());
                emit message(why.isEmpty() ? tr("Couldn't unmount %1.").arg(it->name) : tr("Couldn't unmount %1: %2").arg(it->name, why));
            }
            m_pending.erase(it);
        }
        rebuild();
    });
    connect(m_src, &KFilePlacesModel::errorMessage, this, [this](const QString &text) {
        if (!text.isEmpty()) {
            emit message(clean(text));
            m_errorAge.start();
        }
    });

    m_usageTimer.setInterval(UsageRefreshMs);
    connect(&m_usageTimer, &QTimer::timeout, this, &PlacesLogic::refreshUsage);

    // The Trash's items are counted by a lister that follows the folder, so
    // the number moves when something is trashed or restored (also from
    // another program).
    m_trash = new KCoreDirLister(this);
    m_trash->setAutoErrorHandlingEnabled(false);
    m_trash->setShowHiddenFiles(true);
    m_trashTimer.setSingleShot(true);
    m_trashTimer.setInterval(150);
    connect(&m_trashTimer, &QTimer::timeout, this, &PlacesLogic::updateTrashCount);
    const auto trashChanged = [this] { m_trashTimer.start(); };
    connect(m_trash, &KCoreDirLister::itemsAdded, this, trashChanged);
    connect(m_trash, &KCoreDirLister::itemsDeleted, this, trashChanged);
    connect(m_trash, &KCoreDirLister::clear, this, trashChanged);
    connect(m_trash, &KCoreDirLister::completed, this, trashChanged);
    m_trash->openUrl(QUrl(QStringLiteral("trash:/")), KCoreDirLister::NoFlags);

    rebuild();
}

// The standard folders of a first run, so the sidebar starts as it always
// has (Home, Recent, Desktop, Documents, ...) and the same list shows in
// every Open and Save dialog. A user whose places file exists is left alone.
void PlacesLogic::seedStandardPlaces()
{
    struct Std {
        QStandardPaths::StandardLocation loc;
        const char *icon;
    };
    const Std folders[] = {
        {QStandardPaths::DesktopLocation, "user-desktop"},
        {QStandardPaths::DocumentsLocation, "folder-documents"},
        {QStandardPaths::DownloadLocation, "folder-download"},
        {QStandardPaths::PicturesLocation, "folder-pictures"},
        {QStandardPaths::MusicLocation, "folder-music"},
        {QStandardPaths::MoviesLocation, "folder-videos"},
    };
    const QUrl home = QUrl::fromLocalFile(QDir::homePath());
    QModelIndex after;
    for (int i = 0; i < m_src->rowCount(); ++i) {
        if (normalized(m_src->url(m_src->index(i, 0))) == normalized(home)) {
            after = m_src->index(i, 0);
        }
    }
    const auto known = [this](const QUrl &u) {
        for (int i = 0; i < m_src->rowCount(); ++i) {
            if (normalized(m_src->url(m_src->index(i, 0))) == normalized(u)) {
                return true;
            }
        }
        return false;
    };
    // Each is added after the one before, so the order is the list's.
    const auto add = [&](const QString &text, const QUrl &url, const QString &icon) {
        if (known(url)) {
            return;
        }
        m_src->addPlace(text, url, icon, QString(), after);
        for (int i = 0; i < m_src->rowCount(); ++i) {
            if (normalized(m_src->url(m_src->index(i, 0))) == normalized(url)) {
                after = m_src->index(i, 0);
                break;
            }
        }
    };
    add(tr("Recent"), QUrl(QStringLiteral("recentlyused:/")), QStringLiteral("document-open-recent"));
    for (const Std &f : folders) {
        const QString path = QStandardPaths::writableLocation(f.loc);
        if (!path.isEmpty() && QFileInfo(path).isDir() && QDir::cleanPath(path) != QDir::cleanPath(QDir::homePath())) {
            add(QFileInfo(path).fileName(), QUrl::fromLocalFile(path), QString::fromLatin1(f.icon));
        }
    }
}

void PlacesLogic::setShowHidden(bool on)
{
    if (m_showHidden != on) {
        m_showHidden = on;
        emit showHiddenChanged();
        emit entriesChanged();
    }
}

void PlacesLogic::scheduleRebuild()
{
    if (!m_rebuildTimer.isActive()) {
        m_rebuildTimer.start();
    }
}

QString PlacesLogic::keyAt(int sourceRow) const
{
    const QModelIndex idx = m_src->index(sourceRow, 0);
    if (!idx.isValid()) {
        return {};
    }
    if (m_src->isDevice(idx)) {
        return m_src->deviceForIndex(idx).udi();
    }
    const QString id = m_src->bookmarkForIndex(idx).metaDataItem(QStringLiteral("ID"));
    return id.isEmpty() ? QStringLiteral("url:") + m_src->url(idx).toString() : id;
}

int PlacesLogic::rowOf(const QString &key) const
{
    for (int i = 0; i < m_entries.size(); ++i) {
        if (m_entries.at(i).key == key) {
            return i;
        }
    }
    return -1;
}

// The place's row in KIO's model now. The row noted at the last rebuild can
// be stale (a drive was plugged in since), so it is checked against the key
// and looked up again if it moved. Invalid when the place is gone.
QModelIndex PlacesLogic::indexOf(const PlaceEntry &e) const
{
    if (keyAt(e.sourceRow) == e.key) {
        return m_src->index(e.sourceRow, 0);
    }
    for (int i = 0; i < m_src->rowCount(); ++i) {
        if (keyAt(i) == e.key) {
            return m_src->index(i, 0);
        }
    }
    return {};
}

void PlacesLogic::rebuild()
{
    m_rebuildTimer.stop();
    QList<PlaceEntry> out;
    int hidden = 0;
    const bool disks = disksInstalled();
    QStringList mountedPaths;
    const int rows = m_src->rowCount();
    for (int i = 0; i < rows; ++i) {
        const QModelIndex idx = m_src->index(i, 0);
        PlaceEntry e;
        e.sourceRow = i;
        e.rawUrl = m_src->url(idx);
        e.url = e.rawUrl;
        const QString scheme = e.rawUrl.scheme();
        e.section = int(rustPlacesSection(int(m_src->groupType(idx)), scheme));
        if (e.section == Unlisted) {
            continue;
        }
        e.key = keyAt(i);
        e.device = m_src->isDevice(idx);
        Solid::Device dev;
        bool storage = false;
        bool player = false;
        if (e.device) {
            dev = m_src->deviceForIndex(idx);
            storage = dev.is<Solid::StorageAccess>();
            player = dev.is<Solid::PortableMediaPlayer>();
        }
        const bool removable = m_src->groupType(idx) == KFilePlacesModel::RemovableDevicesType;
        const uint32_t flags = (e.device ? 1u : 0u) | (storage ? 2u : 0u) | (removable ? 4u : 0u) | (player ? 8u : 0u);
        e.kind = int(rustPlacesKind(uint32_t(e.section), scheme, flags));
        e.text = clean(m_src->text(idx));
        e.iconName = m_src->data(idx, KFilePlacesModel::IconNameRole).toString();
        e.hidden = m_src->isHidden(idx);
        if (e.hidden) {
            ++hidden;
        }
        // The Network place is KDE's remote:/ wizard folder; Files opens the
        // network browser.
        if (e.kind == NetworkPlace && scheme == QLatin1String("remote")) {
            e.url = QUrl(QStringLiteral("network:/"));
        }
        if (storage) {
            const auto acc = m_src->deviceAccessibility(idx);
            e.mounted = acc == KFilePlacesModel::Accessible || acc == KFilePlacesModel::TeardownInProgress;
            e.busy = acc == KFilePlacesModel::SetupInProgress || acc == KFilePlacesModel::TeardownInProgress;
            e.setupNeeded = acc == KFilePlacesModel::SetupNeeded || acc == KFilePlacesModel::SetupInProgress;
            if (e.setupNeeded) {
                // Not mounted: there is no folder to show yet.
                e.url = QUrl();
            }
            if (e.mounted && diskLocal(e.url)) {
                const QString path = e.url.toLocalFile();
                mountedPaths << path;
                const auto it = m_usage.constFind(path);
                if (it != m_usage.constEnd()) {
                    e.usagePercent = telamon_places_usage_percent(it->total, it->free);
                    if (e.usagePercent >= 0) {
                        e.usageText = tr("%1 free of %2").arg(KIO::convertSize(KIO::filesize_t(it->free)), KIO::convertSize(KIO::filesize_t(it->total)));
                    }
                }
            }
        }
        if (e.kind == TrashPlace) {
            e.value = rustPlacesText(2, QString(), QString(), quint64(m_trashCount));
            e.tooltip = rustPlacesText(3, QString(), QString(), quint64(m_trashCount));
        } else if (!e.usageText.isEmpty()) {
            e.tooltip = e.text + QStringLiteral(": ") + e.usageText;
        } else if (e.kind == Phone) {
            e.tooltip = e.text;
        }
        e.actions = telamon_places_actions(uint32_t(e.kind), (e.hidden ? 1u : 0u) | (e.mounted ? 2u : 0u) | (m_trashCount == 0 ? 4u : 0u) | (disks ? 8u : 0u));
        out.append(e);
    }

    if (hidden != m_hiddenCount) {
        m_hiddenCount = hidden;
        emit hiddenCountChanged();
    }
    // With nothing left hidden the toggle is gone: it must not stay on.
    if (hidden == 0 && m_showHidden) {
        setShowHidden(false);
    }
    const bool changed = out != m_entries;
    if (changed) {
        m_entries = out;
    }
    // The mounted disks' usage is read again when the set of them changes.
    mountedPaths.sort();
    if (mountedPaths != m_lastMounted) {
        m_lastMounted = mountedPaths;
        refreshUsage();
    }
    if (changed) {
        emit entriesChanged();
    }
    checkPending();
}

// A mount or unmount the user asked for has finished when the drive's state
// has changed (the signals Solid sends are not sent by every backend).
// A mount or unmount the user asked for has finished when the drive's state
// has changed (the signals Solid sends are not sent by every backend).
void PlacesLogic::checkPending()
{
    const QStringList keys = m_pending.keys();
    for (const QString &key : keys) {
        const Pending p = m_pending.value(key);
        const int row = rowOf(key);
        if (row < 0) {
            // The drive was taken out meanwhile.
            m_pending.remove(key);
            continue;
        }
        const PlaceEntry &e = m_entries.at(row);
        if (e.busy) {
            continue;
        }
        if (!p.unmount && e.mounted && e.url.isValid()) {
            m_pending.remove(key);
            emit openRequested(e.url, p.newTab);
        } else if (p.unmount && !e.mounted) {
            m_pending.remove(key);
            emit message(rustPlacesText(5, p.name, QString(), quint64(p.kind)));
        }
    }
}

bool PlacesLogic::quietAfterError() const
{
    return m_errorAge.isValid() && m_errorAge.elapsed() < 1000;
}

// Remembers a mount or unmount, and gives up on it after a while.
void PlacesLogic::addPending(const Pending &p)
{
    Pending q = p;
    q.serial = ++m_pendingSerial;
    m_pending.insert(q.key, q);
    QPointer<PlacesLogic> self(this);
    QTimer::singleShot(PendingMs, this, [self, key = q.key, serial = q.serial] {
        if (!self) {
            return;
        }
        const auto it = self->m_pending.constFind(key);
        if (it != self->m_pending.constEnd() && it->serial == serial) {
            emit self->message(it->unmount ? tr("Couldn't unmount %1.").arg(it->name) : tr("Couldn't mount %1.").arg(it->name));
            self->m_pending.erase(it);
            self->scheduleRebuild();
        }
    });
}

void PlacesLogic::refreshUsage()
{
    QStringList paths;
    for (const PlaceEntry &e : std::as_const(m_entries)) {
        if (e.device && e.mounted && diskLocal(e.url)) {
            paths << e.url.toLocalFile();
        }
    }
    if (paths.isEmpty()) {
        m_usageTimer.stop();
        return;
    }
    if (!m_usageTimer.isActive()) {
        m_usageTimer.start();
    }
    const int serial = ++m_usageSerial;
    QPointer<PlacesLogic> self(this);
    // statfs can hang on a stuck disk: never on the GUI thread.
    QThreadPool::globalInstance()->start([self, paths, serial] {
        QHash<QString, Usage> got;
        for (const QString &p : paths) {
            const QStorageInfo info(p);
            if (info.isValid() && info.isReady()) {
                got.insert(p, Usage{info.bytesTotal(), info.bytesAvailable()});
            }
        }
        QMetaObject::invokeMethod(
            self.data(),
            [self, got, serial] {
                if (!self || serial != self->m_usageSerial) {
                    return;
                }
                self->m_usage = got;
                self->scheduleRebuild();
            },
            Qt::QueuedConnection);
    });
}

void PlacesLogic::updateTrashCount()
{
    const int n = int(m_trash->items(KCoreDirLister::AllItems).size());
    if (n != m_trashCount) {
        m_trashCount = n;
        emit trashCountChanged();
        scheduleRebuild();
    }
}

bool PlacesLogic::disksInstalled() const
{
    const QStringList ids = rustPlacesText(6).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const QString &id : ids) {
        if (!QStandardPaths::locate(QStandardPaths::ApplicationsLocation, id).isEmpty()) {
            return true;
        }
    }
    const QStringList programs = rustPlacesText(7).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const QString &p : programs) {
        if (!QStandardPaths::findExecutable(p).isEmpty()) {
            return true;
        }
    }
    return false;
}

void PlacesLogic::open(const QString &key, bool newTab)
{
    const int row = rowOf(key);
    if (row < 0) {
        return;
    }
    const PlaceEntry e = m_entries.at(row);
    if (e.device && e.setupNeeded && e.url.isEmpty()) {
        if (e.busy) {
            return;
        }
        addPending({key, newTab, 0, false, e.kind, e.text});
        const QModelIndex idx = indexOf(e);
        if (idx.isValid()) {
            m_src->requestSetup(idx);
        }
        // A backend that answers at once has changed the drive already.
        scheduleRebuild();
        return;
    }
    if (!e.url.isValid() || e.url.isEmpty()) {
        emit message(tr("Can't open %1.").arg(e.text));
        return;
    }
    emit openRequested(e.url, newTab);
}

void PlacesLogic::unmount(const QString &key)
{
    const int row = rowOf(key);
    if (row < 0) {
        return;
    }
    const PlaceEntry e = m_entries.at(row);
    if (!e.device || !e.mounted || e.busy) {
        return;
    }
    addPending({key, false, 0, true, e.kind, e.text});
    const QModelIndex idx = indexOf(e);
    if (idx.isValid()) {
        m_src->requestTeardown(idx);
    }
    scheduleRebuild();
}

void PlacesLogic::rename(const QString &key, const QString &text)
{
    const int row = rowOf(key);
    const QString name = rustPlacesText(0, text);
    if (row < 0 || name.isEmpty()) {
        return;
    }
    const PlaceEntry &e = m_entries.at(row);
    if (!(e.actions & ActRename)) {
        return;
    }
    const QModelIndex idx = indexOf(e);
    if (idx.isValid()) {
        m_src->editPlace(idx, name, e.rawUrl, e.iconName);
    }
}

void PlacesLogic::setHidden(const QString &key, bool hidden)
{
    const int row = rowOf(key);
    const QModelIndex idx = row >= 0 ? indexOf(m_entries.at(row)) : QModelIndex();
    if (idx.isValid()) {
        m_src->setPlaceHidden(idx, hidden);
    }
}

void PlacesLogic::remove(const QString &key)
{
    const int row = rowOf(key);
    const QModelIndex idx = row >= 0 && (m_entries.at(row).actions & ActRemove) ? indexOf(m_entries.at(row)) : QModelIndex();
    if (idx.isValid()) {
        m_src->removePlace(idx);
    }
}

void PlacesLogic::moveTo(const QString &src, const QString &dst)
{
    const int a = rowOf(src);
    const int b = rowOf(dst);
    // Only places that sit in the list by the user's order take part (not a
    // drive or the Trash, whose rows are Solid's).
    if (a < 0 || b < 0 || !(m_entries.at(a).actions & ActReorder) || !(m_entries.at(b).actions & ActReorder)) {
        return;
    }
    const QModelIndex from = indexOf(m_entries.at(a));
    const QModelIndex to = indexOf(m_entries.at(b));
    if (!from.isValid() || !to.isValid()) {
        return;
    }
    const int64_t row = telamon_places_reorder_row(size_t(from.row()), size_t(to.row()));
    if (row >= 0) {
        m_src->movePlace(from.row(), int(row));
    }
}

bool PlacesLogic::sameLocation(const QUrl &a, const QUrl &b) const
{
    return a.isValid() && b.isValid() && normalized(a) == normalized(b);
}

bool PlacesLogic::isPinned(const QUrl &url) const
{
    for (int i = 0; i < m_src->rowCount(); ++i) {
        if (sameLocation(m_src->url(m_src->index(i, 0)), url)) {
            return true;
        }
    }
    return false;
}

int PlacesLogic::nearlyFullPercent() const
{
    return telamon_places_nearly_full();
}

QModelIndex PlacesLogic::indexForUrl(const QUrl &url) const
{
    for (int i = 0; i < m_src->rowCount(); ++i) {
        if (sameLocation(m_src->url(m_src->index(i, 0)), url)) {
            return m_src->index(i, 0);
        }
    }
    return {};
}

// Pins folders, each after the one before. A folder that is a place already
// is left (and shown again if it was hidden).
void PlacesLogic::pinDirs(const QList<QUrl> &dirs, const QString &afterKey)
{
    QModelIndex after;
    const int row = rowOf(afterKey);
    if (row >= 0 && (m_entries.at(row).actions & ActReorder)) {
        after = indexOf(m_entries.at(row));
    } else {
        // At the end of the folders.
        for (const PlaceEntry &e : std::as_const(m_entries)) {
            if (e.section == Favourites && e.kind == Folder) {
                after = indexOf(e);
            }
        }
    }
    for (const QUrl &url : dirs) {
        const QString label = rustPlacesText(1, url.toString(QUrl::FullyEncoded | QUrl::RemovePassword), QDir::homePath());
        const QModelIndex have = indexForUrl(url);
        if (have.isValid()) {
            if (m_src->isHidden(have)) {
                m_src->setPlaceHidden(have, false);
                emit message(tr("%1 is back in the sidebar.").arg(label));
            } else {
                emit message(tr("%1 is already in the sidebar.").arg(label));
            }
            continue;
        }
        if (!rustPlacesPinnable(url.scheme())) {
            emit message(tr("%1 can't be pinned to the sidebar.").arg(label));
            continue;
        }
        m_src->addPlace(label, url, KIO::iconNameForUrl(url), QString(), after);
        const QModelIndex added = indexForUrl(url);
        if (added.isValid()) {
            after = added;
        }
    }
}

namespace
{
// The answers of the stat jobs of one drop.
struct DropState {
    int left = 0;
    QList<QUrl> dirs;
    QList<QUrl> files;
    int failed = 0;
};
}

void PlacesLogic::handleDrop(const QList<QUrl> &urls, const QString &target, bool copy)
{
    const int row = rowOf(target);
    if (row < 0 || urls.isEmpty()) {
        return;
    }
    const PlaceEntry t = m_entries.at(row);
    if (t.kind == TrashPlace) {
        emit trashDropped(urls);
        return;
    }
    // A big drop is files being moved: every one goes, none is looked at.
    if (urls.size() > MaxPinsPerDrop) {
        if ((t.actions & ActAcceptsFiles) && t.url.isValid()) {
            emit filesDropped(urls, t.url, copy);
        } else {
            emit message(tr("Drop a folder here to pin it. Files can't go into %1.").arg(t.text));
        }
        return;
    }
    auto state = std::make_shared<DropState>();
    state->left = int(urls.size());
    QPointer<PlacesLogic> self(this);
    for (int i = 0; i < state->left; ++i) {
        const QUrl url = urls.at(i);
        auto *job = KIO::stat(url, KIO::StatJob::SourceSide, KIO::StatBasic, KIO::HideProgressInfo);
        connect(job, &KJob::result, this, [self, job, url, state, t, copy] {
            if (job->error()) {
                ++state->failed;
            } else if (job->statResult().isDir()) {
                state->dirs.append(url);
            } else {
                state->files.append(url);
            }
            if (--state->left > 0 || !self) {
                return;
            }
            if (!state->dirs.isEmpty()) {
                self->pinDirs(state->dirs, t.key);
            }
            if (!state->files.isEmpty()) {
                if ((t.actions & ActAcceptsFiles) && t.url.isValid()) {
                    emit self->filesDropped(state->files, t.url, copy);
                } else if (state->dirs.isEmpty()) {
                    emit self->message(tr("Drop a folder here to pin it. Files can't go into %1.").arg(t.text));
                }
            }
            if (state->failed > 0 && state->dirs.isEmpty() && state->files.isEmpty()) {
                emit self->message(tr("Couldn't read what was dropped."));
            }
        });
        job->start();
    }
}

void PlacesLogic::pinFolder(const QUrl &url)
{
    if (!url.isValid()) {
        return;
    }
    auto *job = KIO::stat(url, KIO::StatJob::SourceSide, KIO::StatBasic, KIO::HideProgressInfo);
    QPointer<PlacesLogic> self(this);
    connect(job, &KJob::result, this, [self, job, url] {
        if (!self) {
            return;
        }
        if (job->error() || !job->statResult().isDir()) {
            emit self->message(tr("Only folders can be pinned."));
            return;
        }
        self->pinDirs({url}, QString());
    });
    job->start();
}

QString PlacesLogic::nameOf(const QString &key) const
{
    const int row = rowOf(key);
    return row < 0 ? QString() : m_entries.at(row).text;
}

QUrl PlacesLogic::urlOf(const QString &key) const
{
    const int row = rowOf(key);
    return row < 0 ? QUrl() : m_entries.at(row).url;
}

bool PlacesLogic::acceptsFiles(const QString &key) const
{
    const int row = rowOf(key);
    return row >= 0 && (m_entries.at(row).actions & ActAcceptsFiles) && m_entries.at(row).url.isValid();
}

QVariantMap PlacesLogic::menuFor(const QString &key) const
{
    QVariantMap m;
    const int row = rowOf(key);
    if (row < 0) {
        return m;
    }
    const PlaceEntry &e = m_entries.at(row);
    // Asked now, not read from the entry: Disks may have been installed since.
    const quint32 a = telamon_places_actions(uint32_t(e.kind), (e.hidden ? 1u : 0u) | (e.mounted ? 2u : 0u) | (m_trashCount == 0 ? 4u : 0u) | (disksInstalled() ? 8u : 0u));
    m.insert(QStringLiteral("name"), e.text);
    m.insert(QStringLiteral("kind"), e.kind);
    m.insert(QStringLiteral("removable"), e.kind == Removable);
    m.insert(QStringLiteral("open"), e.url.isValid() || e.setupNeeded);
    m.insert(QStringLiteral("newTab"), bool(a & ActNewTab) && (e.url.isValid() || e.setupNeeded));
    m.insert(QStringLiteral("mount"), bool(a & ActMount) && !e.busy);
    m.insert(QStringLiteral("unmount"), bool(a & ActUnmount) && !e.busy);
    m.insert(QStringLiteral("openInDisks"), bool(a & ActOpenInDisks));
    m.insert(QStringLiteral("emptyTrash"), bool(a & ActEmptyTrash));
    m.insert(QStringLiteral("rename"), bool(a & ActRename));
    m.insert(QStringLiteral("hide"), bool(a & ActHide));
    m.insert(QStringLiteral("unhide"), bool(a & ActUnhide));
    m.insert(QStringLiteral("remove"), bool(a & ActRemove));
    return m;
}

// ---- Telamon Disks ----

void PlacesLogic::openInDisks(const QString &key)
{
    const int row = rowOf(key);
    if (row < 0 || !(m_entries.at(row).actions & ActOpenInDisks) || !disksInstalled()) {
        return;
    }
    const QString udi = key;
    // The deep link first (Disks answers on the session bus, started by D-Bus
    // activation if it is not running); the program itself if that fails.
    struct Target {
        const char *service;
        const char *path;
        const char *iface;
    };
    static const Target targets[] = {
        {"net.eterneon.telamon.disks", "/net/eterneon/telamon/disks", "net.eterneon.telamon.Disks1"},
        {"net.eterneon.atlas.disks", "/net/eterneon/atlas/disks", "net.eterneon.atlas.Disks1"},
    };
    auto attempt = std::make_shared<std::function<void(int)>>();
    QPointer<PlacesLogic> self(this);
    *attempt = [self, udi, attempt](int i) {
        if (!self) {
            return;
        }
        if (i >= int(std::size(targets))) {
            // No answer on the bus: start the program, if there is one.
            const QStringList programs = rustPlacesText(7).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
            for (const QString &p : programs) {
                const QString exe = QStandardPaths::findExecutable(p);
                if (!exe.isEmpty() && QProcess::startDetached(exe, {})) {
                    return;
                }
            }
            emit self->message(tr("Telamon Disks could not be started."));
            return;
        }
        QDBusMessage call = QDBusMessage::createMethodCall(QString::fromLatin1(targets[i].service), QString::fromLatin1(targets[i].path),
                                                           QString::fromLatin1(targets[i].iface), QStringLiteral("ShowDevice"));
        call << udi;
        auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call, 5000), self);
        connect(w, &QDBusPendingCallWatcher::finished, self, [attempt, i, w] {
            const bool failed = w->isError();
            w->deleteLater();
            if (failed) {
                (*attempt)(i + 1);
            }
        });
    };
    (*attempt)(0);
}

// ---- Trash ----

void PlacesLogic::requestEmptyTrash()
{
    if (m_askingTrash) {
        return;
    }
    m_askingTrash = true;
    auto *job = KIO::directorySize(QUrl(QStringLiteral("trash:/")));
    // A Trash that is slow to measure doesn't hold the question back for long.
    auto *timer = new QTimer(job);
    timer->setSingleShot(true);
    connect(timer, &QTimer::timeout, job, [job] { job->kill(KJob::EmitResult); });
    timer->start(TrashSizeMs);
    QPointer<PlacesLogic> self(this);
    connect(job, &KJob::result, this, [self, job] {
        if (!self) {
            return;
        }
        self->m_askingTrash = false;
        const QString size = job->error() ? QString() : KIO::convertSize(job->totalSize());
        const int count = self->m_trashCount;
        emit self->emptyTrashAsk(count, rustPlacesText(4, size, QString(), quint64(count)));
    });
}
