#include "FileActions.h"

#include "ActionsLogic.h"
#include "ArchiveClient.h"
#include "ArchiveGuard.h"
#include "MenuPrefs.h"
#include "OpsBridge.h"
#include "PlacesLogic.h"
#include "PropsBridge.h"
#include "RustBridge.h"
#include "SearchController.h"
#include "TagLogic.h"

#include <KConfigGroup>
#include <KDesktopFile>
#include <KFileItemActions>
#include <KFileItemListProperties>
#include <KIO/JobUiDelegateFactory>
#include <KIO/OpenUrlJob>
#include <KIO/Paste>
#include <KJobWindows>
#include <KProtocolManager>
#include <KSharedConfig>
#include <KTerminalLauncherJob>
#include <KUrlMimeData>
#include <KWaylandExtras>
#include <KWindowSystem>

#include <QApplication>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDir>
#include <QFileInfo>
#include <QDrag>
#include <QDropEvent>
#include <QFuture>
#include <QImageReader>
#include <QFutureWatcher>
#include <QMenu>
#include <QMimeData>
#include <QMimeDatabase>
#include <QPainter>
#include <QPixmap>
#include <QSet>
#include <QStandardPaths>
#include <QThreadPool>
#include <QTimer>

#include <sys/stat.h>

#include <algorithm>
#include <functional>
#include <iterator>
#include <memory>


namespace
{
// A folder path from the user's input, kept to what KIO can name.
QUrl childUrl(const QUrl &dir, const QString &name)
{
    QUrl u = dir;
    u.setPath(QDir::cleanPath(dir.path() + QLatin1Char('/') + name));
    return u;
}

// Whether `name` is in the folder `dir` on this computer, listed or not (a
// hidden file, one the lister hasn't reached). One lstat; a folder on a
// server is the lister's and KIO's to answer.
bool existsOnDisk(const QUrl &dir, const QString &name)
{
    if (!dir.isLocalFile()) {
        return false;
    }
    const QFileInfo info(QDir(dir.toLocalFile()).filePath(name));
    return info.exists() || info.isSymLink();
}

// A drop that changes nothing: back into the folder the items are in, or onto
// one of them.
bool dropIsPointless(const QList<QUrl> &urls, const QUrl &destination)
{
    bool anywhere = false;
    for (const QUrl &u : urls) {
        if (u == destination) {
            return true;
        }
        if (u.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash) != destination.adjusted(QUrl::StripTrailingSlash)) {
            anywhere = true;
        }
    }
    return !anywhere;
}
}

FileActions::FileActions(QObject *parent)
    : QObject(parent)
    , m_ops(new OperationQueue(this))
{
    connect(QApplication::clipboard(), &QClipboard::dataChanged, this, &FileActions::canPasteChanged);
    connect(QApplication::clipboard(), &QClipboard::dataChanged, this, &FileActions::clipboardChanged);
    connect(m_ops, &OperationQueue::jobFinished, this, &FileActions::jobFinished);
    connect(m_ops, &OperationQueue::resultsReady, this, &FileActions::resultsReady);
    // Tags, ratings and permissions changed (also by an undo): every folder
    // shown reads the items again.
    connect(m_ops, &OperationQueue::attributesChanged, this, [](const QList<QUrl> &urls) {
        FolderModel::invalidateAttributes(urls);
        // The index sees an attribute change on its own (inotify); this makes it sure.
        SearchService::instance()->notifyChanged(urls);
        TagLogic::noteChanged();
    });
    m_ops->setArchiveProbe([this] { return archiveInstalled(); });
    connect(&m_templateWatch, &QFileSystemWatcher::directoryChanged, this, &FileActions::loadTemplates);
    clipboardChanged();
    loadTemplates();
}

FileActions::~FileActions()
{
    delete m_scratch;
}

// The items cut (here or in another app) are dimmed in every folder until they
// are pasted or the clipboard holds something else.
void FileActions::clipboardChanged()
{
    QSet<QString> keys;
    const QMimeData *md = QApplication::clipboard()->mimeData();
    if (md && md->hasUrls() && KIO::isClipboardDataCut(md)) {
        // Another program owns the clipboard: only so many are dimmed (an
        // operation takes no more than that either).
        int n = 0;
        for (const QUrl &u : KUrlMimeData::urlsFromMimeData(md)) {
            if (++n > OperationQueue::MaxItemsPerOperation) {
                break;
            }
            keys.insert(u.adjusted(QUrl::StripTrailingSlash).toString(QUrl::FullyEncoded));
        }
    }
    FolderModel::setCutKeys(keys);
    if (m_cutCount != int(keys.size())) {
        m_cutCount = int(keys.size());
        Q_EMIT cutChanged();
    }
}

void FileActions::setFolder(FolderModel *f)
{
    if (m_folder != f) {
        m_folder = f;
        Q_EMIT folderChanged();
    }
}

void FileActions::setWindow(QQuickWindow *w)
{
    if (m_window != w) {
        m_window = w;
        Q_EMIT windowChanged();
    }
}

bool FileActions::canPaste() const
{
    const QMimeData *md = QApplication::clipboard()->mimeData();
    return md && (md->hasUrls() || md->hasText() || md->hasImage());
}

void FileActions::openUrls(const QList<QUrl> &urls)
{
    // An archive on this computer opens as a folder (read-only); "Open With"
    // still gives Telamon Archive's window.
    if (urls.size() == 1) {
        const KFileItem item = itemsOf(urls).value(0);
        const QUrl inside = !item.isNull() && !item.isDir() ? browseUrl(urls.first()) : QUrl();
        if (inside.isValid()) {
            // An encrypted zip lists but can't be read through KIO: say so
            // instead of showing files that are not what they seem.
            const QUrl file = urls.first();
            QPointer<FileActions> self(this);
            ArchiveCheck::findEncryptedZip({file}, this, [self, inside](const QString &name, bool unknown) {
                if (!self) {
                    return;
                }
                if (name.isEmpty()) {
                    Q_EMIT self->navigateRequested(inside);
                } else {
                    Q_EMIT self->failed(unknown ? ArchiveCheck::undecidedText(name, self->archiveInstalled()) : ArchiveCheck::needsPasswordText(name, self->archiveInstalled()));
                }
            });
            return;
        }
    }
    for (const QUrl &url : urls.mid(0, 64)) {
        auto *job = new KIO::OpenUrlJob(url);
        job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
        if (m_window) {
            KJobWindows::setWindow(job, m_window);
        }
        // "Run or open?" for executables and scripts, the trust prompt for
        // untrusted .desktop files. Never weakened here.
        job->setShowOpenOrExecuteDialog(true);
        job->setRunExecutables(false);
        job->start();
    }
}

void FileActions::copy(const QList<QUrl> &urls, bool cut)
{
    if (urls.isEmpty()) {
        return;
    }
    auto *md = new QMimeData;
    KUrlMimeData::setUrls(urls, urls, md);
    KIO::setClipboardDataCut(md, cut);
    QApplication::clipboard()->setMimeData(md);
}

void FileActions::paste(const QUrl &destination)
{
    const QUrl dest = destination.isEmpty() && m_folder ? m_folder->url() : destination;
    const QMimeData *md = QApplication::clipboard()->mimeData();
    if (!md || dest.isEmpty()) {
        return;
    }
    const QList<QUrl> urls = KUrlMimeData::urlsFromMimeData(md);
    if (!urls.isEmpty()) {
        const bool cut = KIO::isClipboardDataCut(md);
        if (!cut) {
            m_ops->transfer(OperationQueue::Copy, urls, dest);
            return;
        }
        // Nothing is taken off the clipboard until the move has happened: a
        // refused or cancelled paste leaves the items waiting.
        m_ops->transfer(OperationQueue::Move, urls, dest, [urls](bool ok) {
            const QMimeData *now = QApplication::clipboard()->mimeData();
            if (ok && now && KIO::isClipboardDataCut(now) && KUrlMimeData::urlsFromMimeData(now) == urls) {
                QApplication::clipboard()->clear();
            }
        });
        return;
    }
    // Text or an image: KIO asks for a file name and writes it.
    m_ops->pasteData(md, dest);
}

void FileActions::trash(const QList<QUrl> &urls)
{
    if (std::any_of(urls.cbegin(), urls.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); })) {
        Q_EMIT failed(tr("Files in an archive can't be moved to the Trash. Extract them first."));
        return;
    }
    m_ops->trash(urls);
}

void FileActions::deleteForGood(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    const QString what = urls.size() == 1 ? tr("\"%1\"").arg(rustDisplayName(OperationQueue::plainName(urls.first()).toUtf8())) : tr("these %1 items").arg(urls.size());
    Q_EMIT deleteRequested(urls, tr("Delete %1 for good? This can't be undone.").arg(what));
}

void FileActions::confirmDelete(const QList<QUrl> &urls)
{
    m_ops->deleteForGood(urls);
}

void FileActions::emptyTrash()
{
    m_ops->emptyTrash();
}

void FileActions::emptyOldTrash(int days)
{
    m_ops->emptyOldTrash(int(telamon_trash_clamp_days(days)));
}

// ---- Putting items back from the Trash ----

void FileActions::restore(const QList<QUrl> &urls)
{
    if (!m_folder || !m_folder->trashTop() || urls.isEmpty()) {
        return;
    }
    QList<OperationQueue::RestoreItem> items;
    int unknown = 0;
    int unsafePlaces = 0;
    QSet<QString> seen;
    // Where the Trash says each item was (one pass over the rows); nothing else is believed.
    const QHash<QUrl, QString> origins = m_folder->originalPathsOf(urls);
    for (const QUrl &u : urls) {
        if (seen.contains(u.toString())) {
            continue;
        }
        seen.insert(u.toString());
        const QString original = origins.value(u);
        if (original.isEmpty() || !QDir::isAbsolutePath(original)) {
            ++unknown;
            continue;
        }
        // The Trash of the home folder is KIO's own; a drive's is whatever the drive holds.
        // Its items are `trash:/<id>-name`, id 0 being the home Trash.
        const QString target = QDir::cleanPath(original);
        const bool fromHome = u.path().startsWith(QLatin1String("/0-"));
        // The rule is about places, and a place has more than one spelling:
        // `/home/u` is `/var/home/u` here, and a link on the drive can lead
        // to the home folder. The target is checked as written and with the
        // links in its existing part resolved, against the home folder as
        // both spellings.
        bool allowed = true;
        if (!fromHome) {
            QString resolved = target;
            QString tail;
            QString probe = target;
            while (!probe.isEmpty() && !QFileInfo::exists(probe) && probe != QLatin1String("/")) {
                tail = QLatin1Char('/') + QFileInfo(probe).fileName() + tail;
                probe = QFileInfo(probe).path();
            }
            const QString real = QFileInfo(probe).canonicalFilePath();
            if (!real.isEmpty()) {
                resolved = QDir::cleanPath(real + tail);
            }
            for (const QString &home : {QDir::homePath(), QFileInfo(QDir::homePath()).canonicalFilePath()}) {
                for (const QString &place : {target, resolved}) {
                    const QByteArray t = QFile::encodeName(place), h = QFile::encodeName(home);
                    allowed = allowed && telamon_trash_restore_allowed(false, reinterpret_cast<const uint8_t *>(t.constData()), size_t(t.size()), reinterpret_cast<const uint8_t *>(h.constData()), size_t(h.size()));
                }
            }
        }
        if (!allowed) {
            ++unsafePlaces;
            continue;
        }
        items.append({u, QUrl::fromLocalFile(target), false});
    }
    if (unsafePlaces > 0) {
        Q_EMIT failed(tr("A drive's Trash asks for this to go into a hidden place in your home folder, so it stays in the Trash. Drag it out to put it where you want it."));
    }
    if (unknown > 0) {
        Q_EMIT failed(unknown == 1 ? tr("The Trash doesn't say where this item was, so it stays in the Trash.")
                                   : tr("The Trash doesn't say where %1 of these items were, so they stay in the Trash.").arg(unknown));
    }
    if (items.isEmpty()) {
        return;
    }
    // What is at the places, and which folders are gone, is read off the GUI thread.
    QPointer<FileActions> self(this);
    QThreadPool::globalInstance()->start([self, items]() mutable {
        QSet<QString> claimed;
        QStringList missing;
        int needFolder = 0;
        for (OperationQueue::RestoreItem &it : items) {
            const QString path = it.target.toLocalFile();
            struct stat st;
            // Taken: something is there (a dangling link too), or an item before this one goes there.
            it.taken = ::lstat(QFile::encodeName(path).constData(), &st) == 0 || claimed.contains(path);
            claimed.insert(path);
            const QString parent = QFileInfo(path).path();
            if (!missing.contains(parent) && !QFileInfo::exists(parent)) {
                missing << parent;
            }
            // Items that wait for a folder to be made, for the question's words.
            if (missing.contains(parent)) {
                ++needFolder;
            }
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, items, missing, needFolder] {
            if (!self) {
                return;
            }
            if (missing.isEmpty()) {
                self->m_ops->restore(items, {});
                return;
            }
            self->m_restoreItems = items;
            self->m_restoreFolders = missing;
            QStringList shown;
            for (const QString &m : missing) {
                shown << rustSearchPathText(QUrl::fromLocalFile(m).toString(QUrl::FullyEncoded), QDir::homePath());
            }
            const QString list = shown.join(QLatin1Char('\n'));
            Q_EMIT self->restoreAsk(rustTrashText(0, list, quint64(needFolder)), rustTrashText(1, list, quint64(needFolder)));
        });
    });
}

void FileActions::confirmRestore()
{
    const auto items = std::exchange(m_restoreItems, {});
    const auto folders = std::exchange(m_restoreFolders, {});
    m_ops->restore(items, folders);
}

void FileActions::cancelRestore()
{
    m_restoreItems.clear();
    m_restoreFolders.clear();
}

// ---- Names: rename, new folder, new file ----
//
// A rename is edited in place in the views (the window decides: `rename`
// emits `renameRequested`); where a name can't be edited in place the window
// asks in a Telamon.Ui dialog (`namePromptRequested`). Either way the name is
// checked as it is typed (`checkName`) and the work is queued like any other.
// A new folder or file is made at once under a free name and edited in place.

void FileActions::rename(const QList<QUrl> &urls)
{
    if (!urls.isEmpty()) {
        Q_EMIT renameRequested(urls);
    }
}

void FileActions::renameWithDialog(const QUrl &url)
{
    if (url.isEmpty()) {
        return;
    }
    const KFileItem item = m_folder ? m_folder->fileItemOf(url) : KFileItem();
    Q_EMIT namePromptRequested({{QStringLiteral("mode"), QStringLiteral("rename")},
                                {QStringLiteral("title"), tr("Rename")},
                                {QStringLiteral("label"), tr("New name:")},
                                {QStringLiteral("initial"), url.adjusted(QUrl::StripTrailingSlash).fileName()},
                                {QStringLiteral("okText"), tr("Rename")},
                                {QStringLiteral("url"), url},
                                {QStringLiteral("isDir"), item.isDir()}});
}

void FileActions::renameTo(const QUrl &url, const QString &name, bool select)
{
    // Checked again here: the window is not the last word on a name.
    if (url.isEmpty() || !rustValidateName(name).ok || name == url.adjusted(QUrl::StripTrailingSlash).fileName()) {
        return;
    }
    QUrl target = url.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    // A hidden file the view doesn't list is as much "already here".
    if (existsOnDisk(target, name)) {
        Q_EMIT failed(tr("\"%1\" is already here. Choose another name.").arg(rustDisplayName(name.toUtf8())));
        return;
    }
    target = childUrl(target, name);
    QPointer<FileActions> self(this);
    m_ops->rename(url, name, [self, target, select](bool ok) {
        if (self && ok && select) {
            Q_EMIT self->resultsReady({target});
        }
    });
}

QVariantMap FileActions::editableName(const QUrl &url, bool isDir) const
{
    const QString name = url.adjusted(QUrl::StripTrailingSlash).fileName();
    const QByteArray bytes = name.toUtf8();
    const size_t stemBytes = telamon_name_stem_len(reinterpret_cast<const uint8_t *>(bytes.constData()), size_t(bytes.size()), isDir);
    // The core counts bytes; the field counts UTF-16 units.
    const qsizetype stem = QString::fromUtf8(bytes.constData(), qsizetype(qMin(stemBytes, size_t(bytes.size())))).size();
    return {{QStringLiteral("name"), name}, {QStringLiteral("stem"), int(stem)}};
}

void FileActions::tell(const QString &text)
{
    Q_EMIT failed(text);
}

// "name", or "name (2)" and on when the folder shown holds it already.
QString FileActions::suggestName(const QUrl &folder, const QString &wanted) const
{
    QPointer<FolderModel> model = m_folder;
    return rustFreeName(wanted, [&](const QString &candidate) { return (model && model->rowOfUrl(childUrl(folder, candidate)) >= 0) || existsOnDisk(folder, candidate); });
}

void FileActions::newFolder()
{
    if (!m_folder || m_folder->searching()) {
        return;
    }
    const QUrl dir = m_folder->url();
    const QString name = suggestName(dir, rustMenuText(4));
    QPointer<FileActions> self(this);
    m_ops->makeFolder(dir, name, [self, dir, name](bool ok) {
        if (self && ok) {
            Q_EMIT self->createdItem(childUrl(dir.adjusted(QUrl::StripTrailingSlash), name));
        }
    });
}

void FileActions::newFile(const QUrl &templateFile)
{
    if (!m_folder || m_folder->searching()) {
        return;
    }
    const QUrl dir = m_folder->url();
    const QString wanted = templateFile.isEmpty() ? rustMenuText(3) : rustMenuText(1, templateFile.fileName());
    const QString name = suggestName(dir, wanted);
    QPointer<FileActions> self(this);
    m_ops->makeFile(dir, name, templateFile, [self, dir, name](bool ok) {
        if (self && ok) {
            Q_EMIT self->createdItem(childUrl(dir.adjusted(QUrl::StripTrailingSlash), name));
        }
    });
}

QVariantMap FileActions::checkName(const QString &name, const QVariantMap &request) const
{
    if (request.value(QStringLiteral("mode")).toString() == QLatin1String("tag")) {
        const QVariantMap c = TagLogic::instance() ? TagLogic::instance()->checkName(name) : QVariantMap{{QStringLiteral("ok"), false}, {QStringLiteral("text"), QString()}};
        return {{QStringLiteral("ok"), c.value(QStringLiteral("ok"))}, {QStringLiteral("text"), c.value(QStringLiteral("text"))}};
    }
    const RustCheck check = rustValidateName(name);
    if (!check.ok) {
        return {{QStringLiteral("ok"), false}, {QStringLiteral("text"), check.text}};
    }
    // Taken by an item that is listed (a hidden one is KIO's to refuse).
    const QUrl url = request.value(QStringLiteral("url")).toUrl().adjusted(QUrl::StripTrailingSlash);
    if (m_folder && !url.isEmpty()) {
        const QUrl target = childUrl(url.adjusted(QUrl::RemoveFilename), name);
        if (target.adjusted(QUrl::StripTrailingSlash) != url && m_folder->rowOfUrl(target) >= 0) {
            return {{QStringLiteral("ok"), false}, {QStringLiteral("text"), tr("\"%1\" is already here. Choose another name.").arg(rustDisplayName(name.toUtf8()))}};
        }
    }
    return {{QStringLiteral("ok"), true}, {QStringLiteral("text"), check.text}};
}

void FileActions::acceptName(const QVariantMap &request, const QString &name)
{
    if (request.value(QStringLiteral("mode")).toString() == QLatin1String("tag")) {
        const QVariantMap c = TagLogic::instance() ? TagLogic::instance()->checkName(name) : QVariantMap();
        if (c.value(QStringLiteral("ok")).toBool()) {
            toggleTag(request.value(QStringLiteral("urls")).value<QList<QUrl>>(), c.value(QStringLiteral("name")).toString(), true);
        }
        return;
    }
    if (request.value(QStringLiteral("mode")).toString() == QLatin1String("rename")) {
        renameTo(request.value(QStringLiteral("url")).toUrl(), name, true);
    }
}

// ---- Batch Rename ----

namespace
{
TelamonBatchSpec batchSpec(const QVariantMap &spec, QByteArray &first, QByteArray &second)
{
    const QString mode = spec.value(QStringLiteral("mode")).toString();
    TelamonBatchSpec s{};
    s.mode = 99;
    if (mode == QLatin1String("replace")) {
        s.mode = 0;
        first = spec.value(QStringLiteral("find")).toString().toUtf8();
        second = spec.value(QStringLiteral("replace")).toString().toUtf8();
    } else if (mode == QLatin1String("number")) {
        s.mode = 1;
        first = spec.value(QStringLiteral("separator")).toString().toUtf8();
    } else if (mode == QLatin1String("case")) {
        s.mode = 2;
    } else if (mode == QLatin1String("text")) {
        s.mode = 3;
        first = spec.value(QStringLiteral("text")).toString().toUtf8();
    }
    s.first = reinterpret_cast<const uint8_t *>(first.constData());
    s.firstLen = size_t(first.size());
    s.second = reinterpret_cast<const uint8_t *>(second.constData());
    s.secondLen = size_t(second.size());
    s.matchCase = spec.value(QStringLiteral("matchCase")).toBool();
    s.regex = spec.value(QStringLiteral("regex")).toBool();
    // A number out of range is the core's to refuse; a negative one is nonsense.
    auto number = [&](const char *key) { return quint64(qMax(qint64(0), spec.value(QLatin1String(key)).toLongLong())); };
    s.start = number("start");
    s.step = number("step");
    s.padding = quint32(qBound(qint64(0), spec.value(QStringLiteral("padding")).toLongLong(), qint64(1000)));
    s.atEnd = spec.value(QStringLiteral("atEnd")).toBool();
    const QString c = spec.value(QStringLiteral("caseMode")).toString();
    s.caseMode = c == QLatin1String("lower") ? 0 : (c == QLatin1String("upper") ? 1 : (c == QLatin1String("title") ? 2 : 3));
    return s;
}
}

// The items in the order the folder shows them (numbering follows it), the
// ones still listed, all in the folder shown.
QList<QUrl> FileActions::inFolderOrder(const QList<QUrl> &urls) const
{
    QList<QPair<int, QUrl>> rows;
    if (!m_folder || m_folder->searching()) {
        return {};
    }
    const QUrl dir = m_folder->url().adjusted(QUrl::StripTrailingSlash);
    for (const QUrl &u : urls) {
        const int row = m_folder->rowOfUrl(u);
        if (row >= 0 && u.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename).adjusted(QUrl::StripTrailingSlash) == dir) {
            rows.append({row, u});
        }
    }
    std::sort(rows.begin(), rows.end(), [](const auto &a, const auto &b) { return a.first < b.first; });
    QList<QUrl> out;
    for (const auto &r : std::as_const(rows)) {
        if (out.isEmpty() || out.last() != r.second) {
            out.append(r.second);
        }
    }
    return out;
}

QVariantMap FileActions::batchPreview(const QList<QUrl> &urls, const QVariantMap &spec) const
{
    const QList<QUrl> ordered = inFolderOrder(urls);
    QVariantMap out{{QStringLiteral("valid"), false},   {QStringLiteral("canApply"), false}, {QStringLiteral("changed"), 0},
                    {QStringLiteral("blocked"), 0},     {QStringLiteral("problem"), QString()}, {QStringLiteral("rows"), QVariantList()}};
    if (ordered.isEmpty()) {
        return out;
    }
    QList<QPair<QString, bool>> names;
    for (const QUrl &u : ordered) {
        names.append({u.adjusted(QUrl::StripTrailingSlash).fileName(), m_folder->fileItemOf(u).isDir()});
    }
    QByteArray first, second;
    const TelamonBatchSpec s = batchSpec(spec, first, second);
    const QUrl dir = m_folder->url().adjusted(QUrl::StripTrailingSlash);
    QPointer<FolderModel> model = m_folder;
    // The list and Apply look at the disk too (hidden files, files not listed
    // yet): a stat for each name that changes, local folders only.
    const RustBatch plan = rustBatchPlan(s, names, [&](const QString &name) { return (model && model->rowOfUrl(childUrl(dir, name)) >= 0) || existsOnDisk(dir, name); });
    if (!plan.valid) {
        return out;
    }
    QVariantList rows;
    for (qsizetype i = 0; i < plan.rows.size(); ++i) {
        rows.append(QVariantMap{{QStringLiteral("old"), rustDisplayName(names.at(i).first.toUtf8())},
                                {QStringLiteral("new"), rustDisplayName(plan.rows.at(i).name.toUtf8())},
                                {QStringLiteral("code"), plan.rows.at(i).code},
                                {QStringLiteral("text"), plan.rows.at(i).text}});
    }
    out[QStringLiteral("valid")] = true;
    out[QStringLiteral("canApply")] = plan.canApply;
    out[QStringLiteral("changed")] = plan.changed;
    out[QStringLiteral("blocked")] = plan.blocked;
    out[QStringLiteral("problem")] = plan.problem;
    out[QStringLiteral("rows")] = rows;
    return out;
}

void FileActions::batchApply(const QList<QUrl> &urls, const QVariantMap &spec)
{
    const QList<QUrl> ordered = inFolderOrder(urls);
    if (ordered.isEmpty()) {
        return;
    }
    QList<QPair<QString, bool>> names;
    for (const QUrl &u : ordered) {
        names.append({u.adjusted(QUrl::StripTrailingSlash).fileName(), m_folder->fileItemOf(u).isDir()});
    }
    QByteArray first, second;
    const TelamonBatchSpec s = batchSpec(spec, first, second);
    const QUrl dir = m_folder->url().adjusted(QUrl::StripTrailingSlash);
    QPointer<FolderModel> model = m_folder;
    const RustBatch plan = rustBatchPlan(s, names, [&](const QString &name) { return (model && model->rowOfUrl(childUrl(dir, name)) >= 0) || existsOnDisk(dir, name); });
    // The folder may have changed since the list was shown: nothing happens
    // unless the plan holds now.
    if (!plan.valid || !plan.canApply) {
        Q_EMIT failed(tr("The names can't be applied now. Look at the list again."));
        return;
    }
    QList<QPair<QUrl, QString>> renames;
    QList<QUrl> results;
    for (qsizetype i = 0; i < ordered.size(); ++i) {
        if (plan.rows.at(i).code != 0) {
            renames.append({ordered.at(i), plan.rows.at(i).name});
            results.append(childUrl(dir, plan.rows.at(i).name));
        }
    }
    QPointer<FileActions> self(this);
    m_ops->renameMany(renames, [self, results](bool ok) {
        if (self && ok) {
            Q_EMIT self->resultsReady(results);
        }
    });
}

void FileActions::undo()
{
    m_ops->undo();
}

void FileActions::redo()
{
    m_ops->redo();
}

void FileActions::showProperties(const QList<QUrl> &urls)
{
    if (urls.isEmpty() && m_folder) {
        Q_EMIT propertiesRequested({m_folder->url()});
    } else if (!urls.isEmpty()) {
        Q_EMIT propertiesRequested(urls.mid(0, 64));
    }
}

// ---- Tags, ratings and permissions ----

void FileActions::toggleTag(const QList<QUrl> &urls, const QString &name, bool on)
{
    if (urls.isEmpty() || name.isEmpty()) {
        return;
    }
    OperationQueue::AttrEdit edit;
    edit.kind = OperationQueue::AttrEdit::Tags;
    (on ? edit.add : edit.remove) << name;
    const QString shown = rustDisplayName(name.toUtf8());
    m_ops->setAttributes(urls, edit, on ? tr("Tag %1 %2").arg(labelOf(urls), shown) : tr("Remove Tag %1 from %2").arg(shown, labelOf(urls)));
}

void FileActions::clearTags(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    OperationQueue::AttrEdit edit;
    edit.kind = OperationQueue::AttrEdit::Tags;
    edit.clearAll = true;
    m_ops->setAttributes(urls, edit, tr("Clear Tags of %1").arg(labelOf(urls)));
}

void FileActions::newTag(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    Q_EMIT namePromptRequested({{QStringLiteral("mode"), QStringLiteral("tag")},
                                {QStringLiteral("title"), tr("New Tag")},
                                {QStringLiteral("label"), tr("Tag name:")},
                                {QStringLiteral("initial"), QString()},
                                {QStringLiteral("okText"), tr("Add Tag")},
                                {QStringLiteral("urls"), QVariant::fromValue(urls)},
                                {QStringLiteral("isDir"), true}});
}

void FileActions::setRating(const QList<QUrl> &urls, int rating)
{
    if (urls.isEmpty()) {
        return;
    }
    OperationQueue::AttrEdit edit;
    edit.kind = OperationQueue::AttrEdit::Rating;
    edit.rating = qBound(0, rating, 10);
    m_ops->setAttributes(urls, edit, edit.rating == 0 ? tr("Clear Rating of %1").arg(labelOf(urls)) : tr("Rate %1").arg(labelOf(urls)));
}

void FileActions::setPermissions(const QList<QUrl> &urls, uint setBits, uint clearBits, bool recursive)
{
    if (urls.isEmpty() || ((setBits | clearBits) & 0777) == 0) {
        return;
    }
    OperationQueue::AttrEdit edit;
    edit.kind = recursive ? OperationQueue::AttrEdit::ModeTree : OperationQueue::AttrEdit::Mode;
    edit.setBits = setBits & 0777;
    edit.clearBits = clearBits & 0777;
    m_ops->setAttributes(urls, edit, tr("Change Permissions of %1").arg(labelOf(urls)));
}

// ---- Quick actions on pictures ----

namespace
{
// The MIME types Qt can read here (plug-ins differ from system to system).
const QSet<QByteArray> &readableMimeTypes()
{
    static const QSet<QByteArray> set = [] {
        QSet<QByteArray> s;
        for (const QByteArray &m : QImageReader::supportedMimeTypes()) {
            s.insert(m);
        }
        return s;
    }();
    return set;
}

// The name of the action's result in words: "Rotate", "Convert to PNG" ...
QString actionTitle(int action, const QString &what)
{
    switch (action) {
    case ImageWork::RotateLeft:
        return FileActions::tr("Rotate %1 Left").arg(what);
    case ImageWork::RotateRight:
        return FileActions::tr("Rotate %1 Right").arg(what);
    case ImageWork::ToPng:
        return FileActions::tr("Convert %1 to PNG").arg(what);
    case ImageWork::ToJpeg:
        return FileActions::tr("Convert %1 to JPEG").arg(what);
    case ImageWork::ToWebp:
        return FileActions::tr("Convert %1 to WebP").arg(what);
    default:
        return FileActions::tr("Combine %1 into PDF").arg(what);
    }
}
}

FileActions::PictureSet FileActions::pictureSet(const QList<QUrl> &urls, int action) const
{
    PictureSet set;
    if (urls.isEmpty()) {
        return set;
    }
    if (urls.size() > int(telamon_image_limit(0))) {
        set.why = tr("Pictures can be changed %1 at a time.").arg(telamon_image_limit(0));
        return set;
    }
    if (action == ImageWork::ToWebp && !ImageWork::canWrite("webp")) {
        set.why = tr("WebP files can't be written on this system.");
        return set;
    }
    const QMimeDatabase db;
    QString dir;
    QList<uint32_t> kinds;
    QByteArray names;
    for (const QUrl &u : urls) {
        if (!u.isLocalFile()) {
            set.why = tr("These actions are for files on this computer.");
            return set;
        }
        const QFileInfo info(u.toLocalFile());
        const QString here = info.absolutePath();
        if (dir.isEmpty()) {
            dir = here;
        } else if (dir != here) {
            set.why = tr("These files are in different folders. Choose files from one folder.");
            return set;
        }
        if (info.isDir()) {
            set.why = tr("A folder can't be changed.");
            return set;
        }
        const QMimeType type = db.mimeTypeForFile(info.fileName(), QMimeDatabase::MatchExtension);
        const QByteArray mime = type.name().toUtf8();
        const uint32_t kind = telamon_image_kind(reinterpret_cast<const uint8_t *>(mime.constData()), size_t(mime.size()));
        if (kind == 0 || !telamon_image_accepts(uint32_t(action), kind)) {
            set.why = action == ImageWork::CombinePdf ? tr("Only pictures and PDFs can be combined.") : tr("Only pictures can be changed this way.");
            return set;
        }
        // A picture Qt can't read here is not offered (a PDF is read by the core).
        if (kind != ImageWork::Pdf && !readableMimeTypes().contains(mime)) {
            set.why = tr("This system can't read one of these pictures.");
            return set;
        }
        if (action != ImageWork::CombinePdf && telamon_image_skips(uint32_t(action), kind)) {
            continue;
        }
        kinds << kind;
        names += QFile::encodeName(info.fileName());
        names.append('\0');
        set.sources.append({info.absoluteFilePath(), int(kind)});
    }
    if (set.sources.isEmpty()) {
        set.why = tr("They are in that format already.");
        return set;
    }
    if (!QFileInfo(dir).isWritable()) {
        set.why = tr("This folder can't be written to.");
        return set;
    }
    if (action == ImageWork::CombinePdf) {
        set.names = {QStringLiteral("Combined.pdf")};
    } else {
        QByteArray out(names.size() * 2 + 512, 0);
        size_t n = telamon_image_names(uint32_t(action), kinds.constData(), size_t(kinds.size()), reinterpret_cast<const uint8_t *>(names.constData()),
                                       size_t(names.size()), reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
        if (n > size_t(out.size())) {
            out.resize(qsizetype(n));
            n = telamon_image_names(uint32_t(action), kinds.constData(), size_t(kinds.size()), reinterpret_cast<const uint8_t *>(names.constData()),
                                    size_t(names.size()), reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()));
        }
        if (n == 0 || n > size_t(out.size())) {
            set.why = tr("The new names couldn't be worked out.");
            return set;
        }
        for (const QByteArray &part : QByteArray(out.constData(), qsizetype(n)).split('\0')) {
            if (!part.isEmpty()) {
                set.names << QFile::decodeName(part);
            }
        }
        if (set.names.size() != set.sources.size()) {
            set.why = tr("The new names couldn't be worked out.");
            return set;
        }
    }
    set.folder = QUrl::fromLocalFile(dir);
    set.ok = true;
    return set;
}

QVariantMap FileActions::pictureMenu(const QList<QUrl> &urls) const
{
    static const char *const keys[] = {"rotateLeft", "rotateRight", "png", "jpeg", "webp", "combine"};
    // The names Settings > Context Menu and Actions hides them by.
    static const char *const hideKeys[] = {"rotateLeft", "rotateRight", "convertPng", "convertJpeg", "convertWebp", "combinePdf"};
    const QList<QByteArray> hidden = MenuPrefs::hiddenText().split('\n');
    QVariantMap out;
    bool any = false;
    for (int a = 0; a <= ImageWork::CombinePdf; ++a) {
        const bool ok = !hidden.contains(hideKeys[a]) && urls.size() <= int(telamon_image_limit(0)) && pictureSet(urls, a).ok;
        out.insert(QLatin1String(keys[a]), ok);
        any = any || ok;
    }
    out.insert(QStringLiteral("any"), any);
    return out;
}

void FileActions::pictureAction(const QList<QUrl> &urls, int action)
{
    const PictureSet set = pictureSet(urls, action);
    if (!set.ok) {
        if (!set.why.isEmpty()) {
            Q_EMIT failed(set.why);
        }
        return;
    }
    const int count = int(set.sources.size());
    const QString what = count == 1 ? rustDisplayName(QFileInfo(set.sources.first().path).fileName().toUtf8())
                                    : (action == ImageWork::CombinePdf ? tr("%n files", "", count) : tr("%n pictures", "", count));
    const QString title = actionTitle(action, what);
    const QString running = [&] {
        switch (action) {
        case ImageWork::RotateLeft:
            return tr("Turning %1 left").arg(what);
        case ImageWork::RotateRight:
            return tr("Turning %1 right").arg(what);
        case ImageWork::CombinePdf:
            return tr("Combining %1 into a PDF").arg(what);
        default:
            return tr("Converting %1").arg(what);
        }
    }();
    const QList<ImageWork::Source> sources = set.sources;
    const QStringList names = set.names;
    m_ops->produce(title, running, set.folder, names, count,
                   [sources, names, action](const QString &dir, const std::atomic<bool> &cancel, const std::function<void(int)> &progress) -> QStringList {
                       QStringList problems;
                       auto shown = [](const QString &path) { return rustDisplayName(QFileInfo(path).fileName().toUtf8()); };
                       if (action == ImageWork::CombinePdf) {
                           int bad = -1;
                           const QString why = ImageWork::combine(sources, QDir(dir).filePath(names.first()), dir, &cancel, &bad);
                           if (!why.isEmpty() && why != QLatin1String("cancelled")) {
                               problems << (bad >= 0 && bad < sources.size() ? shown(sources.at(bad).path) + QStringLiteral(": ") : QString()) + why;
                           }
                           progress(int(sources.size()));
                           return problems;
                       }
                       for (int i = 0; i < sources.size() && !cancel.load(); ++i) {
                           const QString out = QDir(dir).filePath(names.at(i));
                           QString why;
                           if (action == ImageWork::RotateLeft || action == ImageWork::RotateRight) {
                               why = ImageWork::rotate(sources.at(i), out, action == ImageWork::RotateRight);
                           } else {
                               why = ImageWork::convert(sources.at(i), out, ImageWork::Action(action));
                           }
                           if (!why.isEmpty()) {
                               problems << shown(sources.at(i).path) + QStringLiteral(": ") + why;
                           }
                           progress(i + 1);
                       }
                       return problems;
                   });
}

void FileActions::transferTo(const QList<QUrl> &urls, const QUrl &folder, bool move)
{
    if (urls.isEmpty() || !folder.isValid()) {
        return;
    }
    if (dropIsPointless(urls, folder)) {
        Q_EMIT failed(tr("The other pane shows the folder these items are in."));
        return;
    }
    // Out of an archive the items can only be copied.
    const bool fromArchive = std::any_of(urls.cbegin(), urls.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); });
    m_ops->transfer(move && !fromArchive ? OperationQueue::Move : OperationQueue::Copy, urls, folder);
}

// What the Tags submenu offers for these items, from what the folder has read
// of their tags (nothing is read here). {available, why, colours: [{name,
// colour, state}], named: [{name, text, state}], hasTags}; state 0 no item has
// the tag, 1 some, 2 all.
QVariantMap FileActions::tagMenu(const QList<QUrl> &urls) const
{
    QString why;
    QList<QStringList> lists;
    bool any = false;
    for (const QUrl &u : urls) {
        if (!u.isLocalFile()) {
            why = tr("This location can't keep tags.");
            lists << QStringList();
            continue;
        }
        const FolderModel::TagInfo info = m_folder ? m_folder->tagInfoOf(u) : FolderModel::TagInfo();
        lists << info.names;
        any = any || !info.names.isEmpty();
        if (info.known && info.status != 0) {
            QString t = QString::fromUtf8(PropsBridge::bytesOf([&](uint8_t *o, size_t c) { return telamon_tags_status_text(uint32_t(info.status), o, c); }));
            if (why.isEmpty() || info.status == 1) {
                why = t;
            }
        }
    }
    QByteArray packed;
    for (qsizetype i = 0; i < lists.size(); ++i) {
        if (i > 0) {
            packed.append('\x1e');
        }
        packed += lists.at(i).join(QLatin1Char('\n')).toUtf8();
    }
    auto stateOf = [&](const QString &name) {
        const QByteArray n = name.toUtf8();
        return int(telamon_tags_have(PropsBridge::p(packed), PropsBridge::n(packed), PropsBridge::p(n), PropsBridge::n(n)));
    };
    QVariantList colours, named;
    QStringList seenNames;
    for (const QVariant &c : TagLogic::instance() ? TagLogic::instance()->colours() : QVariantList()) {
        const QVariantMap m = c.toMap();
        colours << QVariantMap{{QStringLiteral("name"), m.value(QStringLiteral("name"))}, {QStringLiteral("colour"), m.value(QStringLiteral("colour"))},
                               {QStringLiteral("state"), stateOf(m.value(QStringLiteral("name")).toString())}};
    }
    // Named tags: those on the items first, then the ones in use elsewhere.
    QStringList candidates;
    for (const QStringList &l : std::as_const(lists)) {
        candidates << l;
    }
    if (TagLogic::instance()) {
        for (const QVariant &t : TagLogic::instance()->sidebarTags()) {
            candidates << t.toMap().value(QStringLiteral("name")).toString();
        }
    }
    for (const QString &name : std::as_const(candidates)) {
        if (!PropsBridge::colourOf(name).isEmpty() || seenNames.contains(name, Qt::CaseInsensitive) || named.size() >= 12) {
            continue;
        }
        seenNames << name;
        named << QVariantMap{{QStringLiteral("name"), name}, {QStringLiteral("text"), rustDisplayName(name.toUtf8())}, {QStringLiteral("state"), stateOf(name)}};
    }
    return {{QStringLiteral("available"), why.isEmpty()}, {QStringLiteral("why"), why},     {QStringLiteral("colours"), colours},
            {QStringLiteral("named"), named},              {QStringLiteral("hasTags"), any}};
}

void FileActions::openTerminal(const QUrl &folder)
{
    const QUrl dir = folder.isEmpty() && m_folder ? m_folder->url() : folder;
    if (!dir.isLocalFile()) {
        Q_EMIT failed(tr("A terminal can only be opened in a folder on this computer."));
        return;
    }
    // The terminal chosen in the system settings (kdeglobals TerminalApplication
    // and TerminalService), started by KIO with the folder as its directory.
    auto *job = new KTerminalLauncherJob(QString());
    job->setWorkingDirectory(dir.toLocalFile());
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    job->start();
}

// ---- The context menus ----
//
// A menu is decided once, here, before the window shows it: the core's rules
// say which entries it has and which are enabled, KFileItemActions supplies
// "Open With" and the service menus, and nothing is looked up again while it
// is open. The window gets plain data; the entries of KFileItemActions run
// through `runMenuAction`, by the id handed out here.

// The hidden keys as a map, for the menu's QML to ask by name ({key: true}).
static QVariantMap hiddenMap(const QList<QByteArray> &keys)
{
    QVariantMap m;
    for (const QByteArray &k : keys) {
        if (!k.isEmpty()) {
            m.insert(QString::fromUtf8(k), true);
        }
    }
    return m;
}

KFileItemList FileActions::itemsOf(const QList<QUrl> &urls) const
{
    KFileItemList items;
    // The type by name only: an item whose type isn't known yet would
    // otherwise read its contents on the GUI thread (slow on a network or
    // FUSE mount). An item no longer listed is left out.
    static const QMimeDatabase mimeDb;
    for (const QUrl &u : urls) {
        const KFileItem it = m_folder ? m_folder->fileItemOf(u) : KFileItem();
        if (it.isNull()) {
            continue;
        }
        if (it.isMimeTypeKnown() || it.isDir()) {
            items << it;
            continue;
        }
        KIO::UDSEntry entry = it.entry();
        entry.replace(KIO::UDSEntry::UDS_MIME_TYPE, mimeDb.mimeTypeForFile(it.name(), QMimeDatabase::MatchExtension).name());
        items << KFileItem(entry, it.url());
    }
    return items;
}

// A menu's actions as plain data: {id, text, icon, enabled, checkable,
// checked, separator, children}. Two levels are kept (a service menu's group,
// "Open With"); anything deeper is listed in the level above. A hostile or
// broken service menu can't make it long: 100 entries a level.
QVariantList FileActions::entriesOf(QMenu *menu, int depth)
{
    QVariantList out;
    auto lastIsSeparator = [&] { return !out.isEmpty() && out.last().toMap().value(QStringLiteral("separator")).toBool(); };
    for (QAction *a : menu->actions()) {
        if (out.size() >= 100) {
            break;
        }
        if (!a->isVisible()) {
            continue;
        }
        if (a->isSeparator()) {
            if (!out.isEmpty() && !lastIsSeparator()) {
                out.append(QVariantMap{{QStringLiteral("separator"), true}});
            }
            continue;
        }
        if (QMenu *sub = a->menu<QMenu *>()) {
            if (depth >= 1) {
                // Too deep: its entries join this level.
                const QVariantList inner = entriesOf(sub, depth + 1);
                for (const QVariant &e : inner) {
                    if (out.size() < 100 && !(e.toMap().value(QStringLiteral("separator")).toBool() && (out.isEmpty() || lastIsSeparator()))) {
                        out.append(e);
                    }
                }
                continue;
            }
            const QVariantList children = entriesOf(sub, depth + 1);
            if (children.isEmpty()) {
                continue;
            }
            out.append(QVariantMap{{QStringLiteral("text"), rustDisplayName(a->text().replace(QLatin1Char('\t'), QLatin1Char(' ')).toUtf8())},
                                   {QStringLiteral("icon"), a->icon().name()},
                                   {QStringLiteral("enabled"), a->isEnabled()},
                                   {QStringLiteral("children"), children}});
            continue;
        }
        const int id = m_nextActionId++;
        m_menuActions.insert(id, a);
        out.append(QVariantMap{{QStringLiteral("id"), id},
                               {QStringLiteral("text"), rustDisplayName(a->text().replace(QLatin1Char('\t'), QLatin1Char(' ')).toUtf8())},
                               {QStringLiteral("icon"), a->icon().name()},
                               {QStringLiteral("enabled"), a->isEnabled()},
                               {QStringLiteral("checkable"), a->isCheckable()},
                               {QStringLiteral("checked"), a->isChecked()}});
    }
    while (lastIsSeparator()) {
        out.removeLast();
    }
    return out;
}

bool FileActions::archiveInstalled() const
{
    const QStringList ids = rustMenuText(5).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const QString &id : ids) {
        if (!QStandardPaths::locate(QStandardPaths::ApplicationsLocation, id).isEmpty()) {
            return true;
        }
    }
    return false;
}

// Where "Open Terminal Here" opens for these items: a folder itself, a file's
// folder, else the folder shown. Empty when that is not on this computer.
QUrl FileActions::terminalFolder(const KFileItemList &items) const
{
    QUrl dir;
    if (items.size() == 1) {
        dir = items.first().isDir() ? items.first().url() : items.first().url().adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash);
    } else if (m_folder && !m_folder->searching()) {
        dir = m_folder->url();
    } else if (!items.isEmpty()) {
        dir = items.first().url().adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash);
    }
    return dir.isLocalFile() ? dir : QUrl();
}

// The previous menu's entries are not needed any more. The actions are not
// deleted at once: one that opened a dialog ("Other Application…") or started
// a job may still be at work, so they go a minute and a half later.
void FileActions::dropScratch()
{
    m_menuActions.clear();
    if (m_scratch) {
        QMenu *old = m_scratch;
        m_scratch = nullptr;
        QTimer::singleShot(90000, old, &QObject::deleteLater);
    }
}

QVariantMap FileActions::itemMenu(const QList<QUrl> &urls)
{
    dropScratch();
    const KFileItemList items = itemsOf(urls);
    if (items.isEmpty()) {
        return {};
    }
    const QString scheme = m_folder ? m_folder->url().scheme() : QString();
    const bool searching = m_folder && m_folder->searching();
    int folders = 0;
    bool local = true, hideable = true, unhideable = true;
    QList<QUrl> itemUrls;
    for (const KFileItem &it : items) {
        itemUrls << it.url();
        folders += it.isDir() ? 1 : 0;
        local = local && it.url().isLocalFile();
        // Listed in the folder's .hidden file: the lister knows, the entry
        // (and so the copy itemsOf made) doesn't; ask the listed item.
        const bool hidden = m_folder->fileItemOf(it.url()).isHidden();
        const bool dot = it.name().startsWith(QLatin1Char('.'));
        hideable = hideable && !hidden;
        unhideable = unhideable && hidden && !dot;
    }

    // What the person hid (Settings > Context Menu and Actions), as the core takes it.
    const QByteArray hiddenText = MenuPrefs::hiddenText();
    const QList<QByteArray> hiddenList = hiddenText.split('\n');
    // "Open With" and the service menus, made now. The menu they are made in
    // is never shown; it only holds the actions until the next menu is made.
    m_scratch = new QMenu;
    auto *actions = new KFileItemActions(m_scratch);
    // What the service menus are offered for is the first items' (as before);
    // the menu's own actions act on every item.
    actions->setItemListProperties(KFileItemListProperties(items.mid(0, 64)));
    connect(actions, &KFileItemActions::error, this, &FileActions::failed);
    auto *openWithMenu = new QMenu(m_scratch);
    actions->insertOpenWithActionsTo(nullptr, openWithMenu, {});
    auto *servicesMenu = new QMenu(m_scratch);
    // Archive replaces Ark's entries, which would show twice; the Activities
    // plugin fills its submenu later ("Loading…"), and a menu must not change
    // once it is shown. Telamon has no Activities.
    // The entries the person hid in Settings are left out as well (service-menu actions by their name, plugins by their id).
    actions->addActionsTo(servicesMenu, KFileItemActions::MenuActionSource::All, {}, MenuPrefs::excludedServices());
    // KFileItemActions gives the preferred application and, in a submenu of
    // its own, the others and "Other Application…": one list for our submenu.
    QVariantList openWith;
    for (const QVariant &e : entriesOf(openWithMenu)) {
        const QVariantList inner = e.toMap().value(QStringLiteral("children")).toList();
        if (inner.isEmpty()) {
            openWith.append(e);
            continue;
        }
        if (!openWith.isEmpty() && !openWith.last().toMap().value(QStringLiteral("separator")).toBool()) {
            openWith.append(QVariantMap{{QStringLiteral("separator"), true}});
        }
        openWith.append(inner);
    }
    QVariantList services = entriesOf(servicesMenu);
    // The actions the person added come first, then the service menus.
    QVariantList custom;
    if (!hiddenList.contains("moreActions")) {
        custom = ActionsLogic::menuEntries(items);
    }
    if (!custom.isEmpty()) {
        if (!services.isEmpty()) {
            custom.append(QVariantMap{{QStringLiteral("separator"), true}});
        }
        services = custom + services;
    }

    const QUrl terminal = terminalFolder(items);
    const bool single = items.size() == 1;
    uint32_t flags = 0;
    auto set = [&flags](bool on, uint32_t bit) { flags |= on ? bit : 0; };
    set(m_folder && m_folder->canWrite(), MenuFlag::Writable);
    set(searching, MenuFlag::Searching);
    set(scheme == QLatin1String("recentlyused"), MenuFlag::Recent);
    set(scheme == QLatin1String("trash"), MenuFlag::InTrash);
    set(m_folder && m_folder->trashTop(), MenuFlag::TrashTop);
    set(local, MenuFlag::Local);
    set(canPaste(), MenuFlag::CanPaste);
    const bool haveArchive = archiveInstalled();
    set(haveArchive, MenuFlag::Archive);
    // Every item an archive Telamon Archive opens (judged by name, as the rest of Files does).
    if (haveArchive) {
        const QStringList mimes = archiveMimeTypes();
        const QMimeDatabase mimeDb;
        bool all = !items.isEmpty() && !mimes.isEmpty();
        for (const KFileItem &it : items) {
            const QMimeType t = mimeDb.mimeTypeForFile(it.name(), QMimeDatabase::MatchExtension);
            bool is = !it.isDir();
            if (is) {
                is = std::any_of(mimes.cbegin(), mimes.cend(), [&t](const QString &m) { return t.inherits(m); });
            }
            all = all && is;
        }
        set(all, MenuFlag::ArchiveItems);
    }
    set(single && folders == 1 && rustPlacesPinnable(items.first().url().scheme()), MenuFlag::Pinnable);
    set(single && folders == 1 && items.first().isWritable() && !(m_folder && m_folder->inArchive()), MenuFlag::FolderWritable);
    set(!openWith.isEmpty(), MenuFlag::OpenWith);
    set(hideable, MenuFlag::Hideable);
    set(!hideable && unhideable, MenuFlag::Unhideable);
    set(!terminal.isEmpty(), MenuFlag::Terminal);

    QVariantMap state;
    for (const auto &e : rustMenuState(true, size_t(items.size()), size_t(folders), flags, hiddenText)) {
        state.insert(e.first, e.second);
    }
    return {{QStringLiteral("state"), state},
            {QStringLiteral("urls"), QVariant::fromValue(itemUrls)},
            {QStringLiteral("single"), single},
            {QStringLiteral("singleFolder"), single && folders == 1},
            {QStringLiteral("pasteIntoFolder"), telamon_menu_paste_into_folder(size_t(items.size()), size_t(folders))},
            {QStringLiteral("terminalFolder"), terminal},
            {QStringLiteral("openWith"), openWith},
            {QStringLiteral("services"), services},
            {QStringLiteral("tags"), tagMenu(itemUrls)},
            {QStringLiteral("pictures"), pictureMenu(itemUrls)},
            {QStringLiteral("hidden"), hiddenMap(hiddenList)}};
}

QVariantMap FileActions::backgroundMenu()
{
    dropScratch();
    if (!m_folder) {
        return {};
    }
    // What Undo and Redo would do, by name; "&" is a mnemonic in a menu.
    auto named = [](const QString &verb, const QString &what) { return what.isEmpty() ? verb : verb + QLatin1Char(' ') + QString(what).replace(QLatin1Char('&'), QStringLiteral("&&")); };
    const QString scheme = m_folder->url().scheme();
    const bool searching = m_folder->searching();
    const QUrl terminal = m_folder->url().isLocalFile() && !searching ? m_folder->url() : QUrl();
    uint32_t flags = 0;
    auto set = [&flags](bool on, uint32_t bit) { flags |= on ? bit : 0; };
    // Search results are from many folders: nothing is made or pasted "here".
    set(m_folder->canWrite() && !searching, MenuFlag::Writable);
    set(searching, MenuFlag::Searching);
    set(scheme == QLatin1String("trash"), MenuFlag::InTrash);
    set(canPaste(), MenuFlag::CanPaste);
    set(rustPlacesPinnable(m_folder->url().scheme()), MenuFlag::Pinnable);
    set(!terminal.isEmpty(), MenuFlag::Terminal);
    set(m_ops->canUndo(), MenuFlag::CanUndo);
    set(m_ops->canRedo(), MenuFlag::CanRedo);
    QVariantMap state;
    for (const auto &e : rustMenuState(false, 0, 0, flags, MenuPrefs::hiddenText())) {
        state.insert(e.first, e.second);
    }
    // The templates read so far (a worker keeps them up to date).
    if (!m_templateWatch.directories().contains(QStandardPaths::writableLocation(QStandardPaths::TemplatesLocation))) {
        loadTemplates();
    }
    QVariantList templates;
    for (const Template &t : std::as_const(m_templates)) {
        templates.append(QVariantMap{{QStringLiteral("label"), rustDisplayName(t.label.toUtf8())}, {QStringLiteral("file"), t.file}});
    }
    return {{QStringLiteral("state"), state},
            {QStringLiteral("hidden"), hiddenMap(MenuPrefs::hiddenText().split('\n'))},
            {QStringLiteral("terminalFolder"), terminal},
            {QStringLiteral("undoText"), named(tr("Undo"), m_ops->undoText())},
            {QStringLiteral("redoText"), named(tr("Redo"), m_ops->redoText())},
            {QStringLiteral("templates"), templates},
            {QStringLiteral("showHidden"), m_folder->showHidden()},
            {QStringLiteral("folder"), m_folder->url()}};
}

// The Templates folder's files, on a worker; again whenever the folder changes.
void FileActions::loadTemplates()
{
    const QString dir = QStandardPaths::writableLocation(QStandardPaths::TemplatesLocation);
    if (dir.isEmpty()) {
        return;
    }
    if (QDir(dir).exists() && !m_templateWatch.directories().contains(dir)) {
        m_templateWatch.addPath(dir);
    }
    const int serial = ++m_templateSerial;
    QPointer<FileActions> self(this);
    QThreadPool::globalInstance()->start([self, dir, serial] {
        // Files only; a link to a file counts, a folder does not.
        const QStringList all = QDir(dir).entryList(QDir::Files | QDir::Readable | QDir::NoDotAndDotDot, QDir::Name);
        const QString picked = rustMenuText(2, all.join(QLatin1Char('\0')));
        QList<Template> list;
        for (const QString &name : picked.split(QLatin1Char('\0'), Qt::SkipEmptyParts)) {
            list.append({rustMenuText(0, name), QUrl::fromLocalFile(dir + QLatin1Char('/') + name)});
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, list, serial] {
            if (self && serial == self->m_templateSerial) {
                self->m_templates = list;
            }
        });
    });
}

void FileActions::runMenuAction(int id)
{
    // Only an entry made for the latest menu runs. It is KFileItemActions' own
    // action: a service menu's command is started by KIO as a program with an
    // argument list, never through a shell.
    if (QAction *a = m_menuActions.value(id).data()) {
        if (a->isEnabled()) {
            a->trigger();
        }
    }
}

void FileActions::openItems(const QList<QUrl> &urls)
{
    const KFileItemList items = itemsOf(urls);
    if (items.size() == 1 && items.first().isDir()) {
        Q_EMIT navigateRequested(items.first().url());
    } else {
        openUrls(urls);
    }
}

void FileActions::openInNewTabs(const QList<QUrl> &urls)
{
    QList<QUrl> folders;
    for (const KFileItem &it : itemsOf(urls)) {
        if (it.isDir()) {
            folders << it.url();
        }
    }
    if (!folders.isEmpty()) {
        Q_EMIT openInNewTabRequested(folders);
    }
}

void FileActions::openLocation(const QList<QUrl> &urls)
{
    if (!urls.isEmpty()) {
        Q_EMIT openLocationRequested(urls);
    }
}

void FileActions::copyPath(const QList<QUrl> &urls)
{
    QStringList lines;
    for (const QUrl &u : urls.mid(0, 1000)) {
        // A file on this computer is its path; any other is its address.
        lines << (u.isLocalFile() ? u.adjusted(QUrl::StripTrailingSlash).toLocalFile() : u.toString(QUrl::PrettyDecoded | QUrl::RemovePassword));
    }
    if (!lines.isEmpty()) {
        QApplication::clipboard()->setText(lines.join(QLatin1Char('\n')));
    }
}

void FileActions::setHidden(const QList<QUrl> &urls, bool hide)
{
    QPointer<FileActions> self(this);
    m_ops->setHidden(urls, hide, [self](bool ok) {
        // KIO lists a folder as it is when asked: ask again.
        if (ok && self && self->m_folder) {
            self->m_folder->refresh();
        }
    });
}

// ---- Telamon Archive ----
//
// Archive's Archive1 over D-Bus (cpp/kio/ArchiveClient.h). Extract Here and
// Compress to ZIP are jobs: Archive returns the job and Files shows it in its
// own queue (progress, pause, cancel) and selects the result. Extract To… and
// Compress… are Archive's dialogs: the call returns a job that waits for the
// dialog, and is followed like the others.

QStringList FileActions::archiveMimeTypes() const
{
    QStringList out;
    for (const QString &id : rustMenuText(5).split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        const QString path = QStandardPaths::locate(QStandardPaths::ApplicationsLocation, id);
        if (path.isEmpty()) {
            continue;
        }
        const KDesktopFile desktop(path);
        out += desktop.desktopGroup().readXdgListEntry(QStringLiteral("MimeType"));
    }
    out.removeDuplicates();
    return out;
}

// The options every call carries. Archive's own progress window stays off when
// Files shows the job (`showProgress` false); a call that makes no job to
// follow leaves it on, or nothing would show. On Wayland the activation token
// is asked for first (it lets Archive's windows take the focus), and the call
// goes ahead without one after half a second.
void FileActions::withArchiveOptions(bool showProgress, std::function<void(QVariantMap)> go)
{
    QVariantMap options;
    if (!showProgress) {
        options.insert(QStringLiteral("show_progress"), false);
    }
    if (m_window && QGuiApplication::platformName() == QLatin1String("xcb")) {
        options.insert(QStringLiteral("parent_window"), QStringLiteral("x11:") + QString::number(m_window->winId(), 16));
    }
    if (!m_window || !KWindowSystem::isPlatformWayland()) {
        go(options);
        return;
    }
    auto *watcher = new QFutureWatcher<QString>(this);
    auto fired = std::make_shared<bool>(false);
    auto finish = [watcher, fired, options, go](const QString &token) mutable {
        if (*fired) {
            return;
        }
        *fired = true;
        QVariantMap o = options;
        if (!token.isEmpty()) {
            o.insert(QStringLiteral("activation_token"), token);
        }
        watcher->deleteLater();
        go(o);
    };
    connect(watcher, &QFutureWatcher<QString>::finished, this, [watcher, finish]() mutable { finish(watcher->result()); });
    watcher->setFuture(KWaylandExtras::xdgActivationToken(m_window, QGuiApplication::desktopFileName()));
    QTimer::singleShot(500, this, [finish]() mutable { finish(QString()); });
}

QString FileActions::labelOf(const QList<QUrl> &urls) const
{
    if (urls.size() == 1) {
        return tr("\"%1\"").arg(rustDisplayName(urls.first().adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()));
    }
    return tr("%n items", "", int(urls.size()));
}

void FileActions::runArchive(const QString &method, const QList<QUrl> &urls, const QVariantList &extra, const QString &title, const QString &running)
{
    if (urls.size() > 1000) {
        Q_EMIT failed(tr("Telamon Archive takes at most 1,000 items at a time."));
        return;
    }
    QStringList files;
    for (const QUrl &u : urls) {
        if (u.isLocalFile()) {
            files << u.toString(QUrl::FullyEncoded);
        }
    }
    if (files.isEmpty()) {
        Q_EMIT failed(tr("Telamon Archive works on files on this computer only."));
        return;
    }
    QVariantList args;
    args << files;
    args << extra;
    QPointer<FileActions> self(this);
    withArchiveOptions(false, [self, method, args, title, running](const QVariantMap &options) {
        if (self) {
            self->m_ops->archive(method, args, options, title, running);
        }
    });
}

void FileActions::extractHere(const QList<QUrl> &urls)
{
    const QString what = labelOf(urls);
    runArchive(QStringLiteral("ExtractHere"), urls, {}, tr("Extract %1").arg(what), tr("Extracting %1").arg(what));
}

void FileActions::compressToZip(const QList<QUrl> &urls)
{
    const QString what = labelOf(urls);
    // An empty destination: Archive names it next to the first item.
    runArchive(QStringLiteral("Compress"), urls, {QStringLiteral("zip"), QString()}, tr("Compress %1").arg(what), tr("Compressing %1").arg(what));
}

// Compress…: Archive's dialog asks for the name, place, format and level. The
// call returns a job that waits for the dialog's answer, so it is followed
// like any other: progress in the queue, and the archive selected at the end.
void FileActions::compress(const QList<QUrl> &urls)
{
    const QString what = labelOf(urls);
    runArchive(QStringLiteral("CompressDialog"), urls.mid(0, 1000), {}, tr("Compress %1").arg(what), tr("Compressing %1").arg(what));
}

// Extract To…: Archive's dialog asks where (no picker of Files' own); the job
// it returns is followed the same way.
void FileActions::extractTo(const QList<QUrl> &urls)
{
    const QString what = labelOf(urls);
    runArchive(QStringLiteral("ExtractAll"), urls.mid(0, 1000), {}, tr("Extract %1").arg(what), tr("Extracting %1").arg(what));
}

void FileActions::extractViewed()
{
    if (!m_folder) {
        return;
    }
    const RustArchiveLocation where = rustArchiveLocate(m_folder->url());
    if (!where.valid) {
        return;
    }
    if (archiveInstalled()) {
        extractTo({where.file});
        return;
    }
    // Without Archive: Files takes everything out itself, into a new folder
    // beside the archive, after looking at what the archive lists.
    m_ops->extractArchive(where.root, where.file.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash), where.folderName, where.name);
}

QUrl FileActions::browseUrl(const QUrl &url) const
{
    if (!url.isLocalFile()) {
        return {};
    }
    const QMimeType type = QMimeDatabase().mimeTypeForFile(url.fileName(), QMimeDatabase::MatchExtension);
    // KIO's own table of what its archive worker opens (kio-extras).
    // Only the types the worker names itself: a .docx or .epub is a zip too
    // (it inherits application/zip), but it is a document, and opens as one.
    const QString protocol = KProtocolManager::protocolForArchiveMimetype(type.name());
    if (protocol.isEmpty() || !rustArchiveScheme(protocol)) {
        return {};
    }
    QUrl browse = url.adjusted(QUrl::StripTrailingSlash);
    browse.setScheme(protocol);
    return browse;
}

void FileActions::startDrag(const QList<QUrl> &urls)
{
    if (urls.isEmpty() || !m_window) {
        return;
    }
    // After the press handler has returned: exec() runs its own event loop.
    QTimer::singleShot(0, this, [this, urls] {
        auto *md = new QMimeData;
        KUrlMimeData::setUrls(urls, urls, md);
        auto *drag = new QDrag(m_window);
        drag->setMimeData(md);
        // The first item's icon, and how many there are.
        const QString iconName = QMimeDatabase().mimeTypeForFile(urls.first().fileName(), QMimeDatabase::MatchExtension).iconName();
        QIcon icon = QIcon::fromTheme(iconName);
        if (icon.isNull()) {
            icon = QIcon::fromTheme(QStringLiteral("text-x-generic"));
        }
        QPixmap pix = icon.pixmap(32);
        if (urls.size() > 1) {
            const qreal dpr = pix.devicePixelRatio();
            QPixmap withCount(pix.size() + QSize(8, 8) * dpr);
            withCount.setDevicePixelRatio(dpr);
            withCount.fill(Qt::transparent);
            QPainter p(&withCount);
            p.drawPixmap(QPointF(0, 8), pix);
            const QString n = urls.size() > 99 ? QStringLiteral("99+") : QString::number(urls.size());
            QFont f = p.font();
            f.setBold(true);
            f.setPixelSize(10);
            p.setFont(f);
            const QRect badge(0, 0, qMax(16, p.fontMetrics().horizontalAdvance(n) + 8), 16);
            p.setRenderHint(QPainter::Antialiasing);
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(0x68, 0x58, 0xE2));
            p.drawRoundedRect(badge, 8, 8);
            p.setPen(Qt::white);
            p.drawText(badge, Qt::AlignCenter, n);
            pix = withCount;
        }
        drag->setPixmap(pix);
        // What is in an archive can only be copied out of it.
        const bool inArchive = std::any_of(urls.cbegin(), urls.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); });
        drag->exec(inArchive ? Qt::CopyAction : (Qt::CopyAction | Qt::MoveAction | Qt::LinkAction), Qt::CopyAction);
    });
}

void FileActions::drop(const QList<QUrl> &urls, const QUrl &destination)
{
    if (urls.isEmpty() || destination.isEmpty()) {
        return;
    }
    if (dropIsPointless(urls, destination)) {
        return;
    }
    // Out of an archive a drop extracts: there is nothing to choose between.
    if (std::any_of(urls.cbegin(), urls.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); })) {
        dropWith(urls, destination, QStringLiteral("copy"));
        return;
    }
    // A key held decides; otherwise the user chooses (Move Here, Copy Here, Link Here).
    const Qt::KeyboardModifiers mods = QGuiApplication::queryKeyboardModifiers();
    if (mods.testFlag(Qt::ControlModifier) && mods.testFlag(Qt::ShiftModifier)) {
        dropWith(urls, destination, QStringLiteral("link"));
    } else if (mods.testFlag(Qt::ControlModifier)) {
        dropWith(urls, destination, QStringLiteral("copy"));
    } else if (mods.testFlag(Qt::ShiftModifier)) {
        dropWith(urls, destination, QStringLiteral("move"));
    } else {
        const QPoint at = m_window ? m_window->mapFromGlobal(QCursor::pos()) : QPoint();
        Q_EMIT dropMenuRequested(urls, destination, at.x(), at.y());
    }
}

void FileActions::dropWith(const QList<QUrl> &urls, const QUrl &destination, const QString &action)
{
    if (urls.isEmpty() || destination.isEmpty() || dropIsPointless(urls, destination)) {
        return;
    }
    const auto kind = action == QLatin1String("copy") ? OperationQueue::Copy : (action == QLatin1String("link") ? OperationQueue::Link : OperationQueue::Move);
    // Files dropped on the Trash are trashed, whatever the key; those in an
    // archive are not Files' to trash.
    if (destination.scheme() == QLatin1String("trash")) {
        if (std::any_of(urls.cbegin(), urls.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); })) {
            Q_EMIT failed(tr("Files in an archive can't be moved to the Trash. Drop them on a folder to extract them."));
            return;
        }
        m_ops->trash(urls);
        return;
    }
    m_ops->transfer(kind, urls, destination);
}

void FileActions::dropTo(const QList<QUrl> &urls, const QUrl &destination, bool copy)
{
    if (urls.isEmpty() || destination.isEmpty() || dropIsPointless(urls, destination)) {
        return;
    }
    m_ops->transfer(copy ? OperationQueue::Copy : OperationQueue::Move, urls, destination);
}

bool FileActions::copyKeyHeld() const
{
    return QGuiApplication::queryKeyboardModifiers().testFlag(Qt::ControlModifier);
}

QVariantMap FileActions::parseAddress(const QString &text) const
{
    const RustCheck r = rustParseAddress(text, m_folder ? m_folder->url().toString(QUrl::FullyEncoded) : QString(), QDir::homePath());
    return {{QStringLiteral("ok"), r.ok}, {QStringLiteral("text"), r.text}};
}

bool FileActions::savedShowHidden() const
{
    return KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"))->group(QStringLiteral("View")).readEntry("ShowHidden", false);
}

void FileActions::saveShowHidden(bool on)
{
    auto cfg = KSharedConfig::openConfig(QStringLiteral("telamon-explorerrc"));
    cfg->group(QStringLiteral("View")).writeEntry("ShowHidden", on);
    cfg->sync();
}
