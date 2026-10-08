#include "FileActions.h"

#include "OpsBridge.h"
#include "PlacesLogic.h"
#include "RustBridge.h"

#include <KConfigGroup>
#include <KFileItemActions>
#include <KFileItemListProperties>
#include <KIO/JobUiDelegateFactory>
#include <KIO/OpenUrlJob>
#include <KIO/Paste>
#include <KJobWindows>
#include <KPropertiesDialog>
#include <KSharedConfig>
#include <KTerminalLauncherJob>
#include <KUrlMimeData>

#include <QApplication>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDir>
#include <QDrag>
#include <QDropEvent>
#include <QMenu>
#include <QMimeData>
#include <QMimeDatabase>
#include <QStandardPaths>
#include <QThreadPool>
#include <QTimer>

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
        for (const QUrl &u : KUrlMimeData::urlsFromMimeData(md)) {
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
    m_ops->trash(urls);
}

void FileActions::deleteForGood(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    const QString what = urls.size() == 1 ? tr("\"%1\"").arg(rustDisplayName(urls.first().adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()))
                                          : tr("these %1 items").arg(urls.size());
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

// ---- Names: rename, new folder, new file ----
//
// The window asks (a Telamon.Ui dialog, `namePromptRequested`), checks the name
// as it is typed (`checkName`) and answers with `acceptName`; the work is
// queued like any other.

void FileActions::prompt(const QVariantMap &request)
{
    Q_EMIT namePromptRequested(request);
}

void FileActions::rename(const QUrl &url)
{
    if (url.isEmpty()) {
        return;
    }
    const KFileItem item = m_folder ? m_folder->fileItemOf(url) : KFileItem();
    prompt({{QStringLiteral("mode"), QStringLiteral("rename")},
            {QStringLiteral("title"), tr("Rename")},
            {QStringLiteral("label"), tr("New name:")},
            {QStringLiteral("initial"), url.adjusted(QUrl::StripTrailingSlash).fileName()},
            {QStringLiteral("okText"), tr("Rename")},
            {QStringLiteral("url"), url},
            {QStringLiteral("isDir"), item.isDir()}});
}

// "name", or "name (2)" and on when the folder shown holds it already.
QString FileActions::suggestName(const QUrl &folder, const QString &wanted) const
{
    QPointer<FolderModel> model = m_folder;
    return rustFreeName(wanted, [&](const QString &candidate) { return model && model->rowOfUrl(childUrl(folder, candidate)) >= 0; });
}

void FileActions::newFolder()
{
    if (!m_folder || m_folder->searching()) {
        return;
    }
    const QUrl dir = m_folder->url();
    prompt({{QStringLiteral("mode"), QStringLiteral("newFolder")},
            {QStringLiteral("title"), tr("New Folder")},
            {QStringLiteral("label"), tr("Folder name:")},
            {QStringLiteral("initial"), suggestName(dir, rustMenuText(4))},
            {QStringLiteral("okText"), tr("Create")},
            {QStringLiteral("dir"), dir},
            {QStringLiteral("isDir"), true}});
}

void FileActions::newFile(const QUrl &templateFile)
{
    if (!m_folder || m_folder->searching()) {
        return;
    }
    const QUrl dir = m_folder->url();
    const QString wanted = templateFile.isEmpty() ? rustMenuText(3) : rustMenuText(1, templateFile.fileName());
    prompt({{QStringLiteral("mode"), QStringLiteral("newFile")},
            {QStringLiteral("title"), tr("New File")},
            {QStringLiteral("label"), tr("File name:")},
            {QStringLiteral("initial"), suggestName(dir, wanted)},
            {QStringLiteral("okText"), tr("Create")},
            {QStringLiteral("dir"), dir},
            {QStringLiteral("url"), templateFile},
            {QStringLiteral("isDir"), false}});
}

QVariantMap FileActions::checkName(const QString &name, const QVariantMap &request) const
{
    const RustCheck check = rustValidateName(name);
    if (!check.ok) {
        return {{QStringLiteral("ok"), false}, {QStringLiteral("text"), check.text}};
    }
    // Taken by an item that is listed (a hidden one is KIO's to refuse).
    const QString mode = request.value(QStringLiteral("mode")).toString();
    const QUrl base = mode == QLatin1String("rename") ? request.value(QStringLiteral("url")).toUrl().adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename)
                                                      : request.value(QStringLiteral("dir")).toUrl();
    if (m_folder && !base.isEmpty()) {
        const QUrl target = childUrl(base, name);
        const bool same = mode == QLatin1String("rename") && target.adjusted(QUrl::StripTrailingSlash) == request.value(QStringLiteral("url")).toUrl().adjusted(QUrl::StripTrailingSlash);
        if (!same && m_folder->rowOfUrl(target) >= 0) {
            return {{QStringLiteral("ok"), false}, {QStringLiteral("text"), tr("\"%1\" is already here. Choose another name.").arg(rustDisplayName(name.toUtf8()))}};
        }
    }
    return {{QStringLiteral("ok"), true}, {QStringLiteral("text"), check.text}};
}

void FileActions::acceptName(const QVariantMap &request, const QString &name)
{
    // Checked again here: the window is not the last word on a name.
    if (!rustValidateName(name).ok) {
        return;
    }
    const QString mode = request.value(QStringLiteral("mode")).toString();
    if (mode == QLatin1String("rename")) {
        const QUrl url = request.value(QStringLiteral("url")).toUrl();
        if (!url.isEmpty() && name != url.adjusted(QUrl::StripTrailingSlash).fileName()) {
            m_ops->rename(url, name);
        }
    } else if (mode == QLatin1String("newFolder")) {
        m_ops->makeFolder(request.value(QStringLiteral("dir")).toUrl(), name);
    } else if (mode == QLatin1String("newFile")) {
        m_ops->makeFile(request.value(QStringLiteral("dir")).toUrl(), name, request.value(QStringLiteral("url")).toUrl());
    }
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
        KPropertiesDialog::showDialog(m_folder->url(), nullptr, false);
    } else if (!urls.isEmpty()) {
        KPropertiesDialog::showDialog(urls.mid(0, 64), nullptr, false);
    }
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
    actions->addActionsTo(servicesMenu, KFileItemActions::MenuActionSource::All, {}, {QStringLiteral("compressfileitemaction"), QStringLiteral("extractfileitemaction"), QStringLiteral("kactivitymanagerd_fileitem_linking_plugin")});
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
    const QVariantList services = entriesOf(servicesMenu);

    const QUrl terminal = terminalFolder(items);
    const bool single = items.size() == 1;
    uint32_t flags = 0;
    auto set = [&flags](bool on, uint32_t bit) { flags |= on ? bit : 0; };
    set(m_folder && m_folder->canWrite(), MenuFlag::Writable);
    set(searching, MenuFlag::Searching);
    set(scheme == QLatin1String("recentlyused"), MenuFlag::Recent);
    set(scheme == QLatin1String("trash"), MenuFlag::InTrash);
    set(local, MenuFlag::Local);
    set(canPaste(), MenuFlag::CanPaste);
    set(archiveInstalled(), MenuFlag::Archive);
    set(single && folders == 1 && rustPlacesPinnable(items.first().url().scheme()), MenuFlag::Pinnable);
    set(single && folders == 1 && items.first().isWritable(), MenuFlag::FolderWritable);
    set(!openWith.isEmpty(), MenuFlag::OpenWith);
    set(hideable, MenuFlag::Hideable);
    set(!hideable && unhideable, MenuFlag::Unhideable);
    set(!terminal.isEmpty(), MenuFlag::Terminal);

    QVariantMap state;
    for (const auto &e : rustMenuState(true, size_t(items.size()), size_t(folders), flags)) {
        state.insert(e.first, e.second);
    }
    return {{QStringLiteral("state"), state},
            {QStringLiteral("urls"), QVariant::fromValue(itemUrls)},
            {QStringLiteral("single"), single},
            {QStringLiteral("singleFolder"), single && folders == 1},
            {QStringLiteral("pasteIntoFolder"), telamon_menu_paste_into_folder(size_t(items.size()), size_t(folders))},
            {QStringLiteral("terminalFolder"), terminal},
            {QStringLiteral("openWith"), openWith},
            {QStringLiteral("services"), services}};
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
    for (const auto &e : rustMenuState(false, 0, 0, flags)) {
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
        lines << (u.isLocalFile() ? u.adjusted(QUrl::StripTrailingSlash).toLocalFile() : u.toString(QUrl::PrettyDecoded));
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

// Telamon Archive's Compress… dialog, over D-Bus (its Archive1 interface takes
// file:// URIs). The new name of the service first, then the old one, which
// Archive keeps for one release.
void FileActions::compress(const QList<QUrl> &urls)
{
    QStringList files;
    for (const QUrl &u : urls.mid(0, 1000)) {
        if (u.isLocalFile()) {
            files << u.toString(QUrl::FullyEncoded);
        }
    }
    if (files.isEmpty()) {
        return;
    }
    QVariantMap options{{QStringLiteral("show_progress"), false}};
    if (m_window && QGuiApplication::platformName() == QLatin1String("xcb")) {
        options.insert(QStringLiteral("parent_window"), QStringLiteral("x11:") + QString::number(m_window->winId(), 16));
    }
    struct Target {
        const char *service;
        const char *path;
        const char *iface;
    };
    static const Target targets[] = {
        {"net.eterneon.telamon.archive", "/net/eterneon/telamon/archive", "net.eterneon.telamon.Archive1"},
        {"net.eterneon.atlas.archive", "/net/eterneon/atlas/archive", "net.eterneon.atlas.Archive1"},
    };
    // Tries the service names in turn, the next only when the one asked has no
    // owner (Archive is not running and can't be started under that name); any
    // other answer is the answer.
    auto attempt = std::make_shared<std::function<void(int)>>();
    std::weak_ptr<std::function<void(int)>> again = attempt;
    QPointer<FileActions> self(this);
    *attempt = [self, files, options, again](int i) {
        if (!self) {
            return;
        }
        if (i >= int(std::size(targets))) {
            Q_EMIT self->failed(tr("Telamon Archive could not be started."));
            return;
        }
        QDBusMessage call = QDBusMessage::createMethodCall(QString::fromLatin1(targets[i].service), QString::fromLatin1(targets[i].path),
                                                           QString::fromLatin1(targets[i].iface), QStringLiteral("CompressDialog"));
        call << files << options;
        auto *w = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(call, 10000), self);
        // The watcher holds the function while the call is out.
        auto keep = again.lock();
        QObject::connect(w, &QDBusPendingCallWatcher::finished, self.data(), [self, keep, i, w] {
            const QDBusError error = w->error();
            const bool failed = w->isError();
            w->deleteLater();
            if (!failed || !self) {
                return;
            }
            if (error.type() == QDBusError::ServiceUnknown || error.type() == QDBusError::NoServer) {
                (*keep)(i + 1);
            } else if (error.name().endsWith(QLatin1String(".TooManyJobs"))) {
                Q_EMIT self->failed(tr("Archive is busy, try again when a job finishes."));
            } else {
                Q_EMIT self->failed(tr("Telamon Archive could not compress these: %1").arg(rustDisplayName(error.message().toUtf8())));
            }
        });
    };
    (*attempt)(0);
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
        drag->setPixmap(QIcon::fromTheme(QStringLiteral("text-x-generic")).pixmap(32));
        drag->exec(Qt::CopyAction | Qt::MoveAction | Qt::LinkAction, Qt::CopyAction);
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
    // Files dropped on the Trash are trashed, whatever the key.
    if (destination.scheme() == QLatin1String("trash")) {
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
