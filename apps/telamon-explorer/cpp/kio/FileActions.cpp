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
#include <QDir>
#include <QDrag>
#include <QDropEvent>
#include <QInputDialog>
#include <QMenu>
#include <QMessageBox>
#include <QMimeData>
#include <QMimeDatabase>
#include <QTimer>


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
    clipboardChanged();
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

// Dialogs are top-level widgets; this makes them belong to the window.
void FileActions::tune(QWidget *dialog)
{
    if (m_window) {
        dialog->winId();
        if (QWindow *h = dialog->windowHandle()) {
            h->setTransientParent(m_window);
        }
    }
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

// Asks for a name and checks it with the core; refusals are shown and asked
// again, warnings need a yes.
void FileActions::askName(const QString &title, const QString &label, const QString &initial, std::function<void(const QString &)> done)
{
    auto *dlg = new QInputDialog;
    dlg->setAttribute(Qt::WA_DeleteOnClose);
    dlg->setWindowTitle(title);
    dlg->setLabelText(label);
    dlg->setTextValue(initial);
    tune(dlg);
    connect(dlg, &QInputDialog::textValueSelected, this, [this, title, label, done](const QString &text) {
        const RustCheck check = rustValidateName(text);
        if (!check.ok) {
            QMessageBox box(QMessageBox::Warning, title, check.text, QMessageBox::Ok);
            tune(&box);
            box.exec();
            askName(title, label, text, done);
            return;
        }
        if (!check.text.isEmpty()) {
            QMessageBox box(QMessageBox::Question, title, check.text + QLatin1String("\n\n") + tr("Use this name anyway?"), QMessageBox::Yes | QMessageBox::No);
            box.setDefaultButton(QMessageBox::No);
            tune(&box);
            if (box.exec() != QMessageBox::Yes) {
                askName(title, label, text, done);
                return;
            }
        }
        done(text);
    });
    dlg->open();
}

void FileActions::rename(const QUrl &url)
{
    if (url.isEmpty()) {
        return;
    }
    askName(tr("Rename"), tr("New name:"), url.fileName(), [this, url](const QString &name) {
        if (name == url.fileName()) {
            return;
        }
        m_ops->rename(url, name);
    });
}

void FileActions::newFolder()
{
    if (!m_folder) {
        return;
    }
    const QUrl dir = m_folder->url();
    askName(tr("New Folder"), tr("Folder name:"), tr("New Folder"), [this, dir](const QString &name) { m_ops->makeFolder(dir, name); });
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

void FileActions::openTerminal()
{
    if (!m_folder || !m_folder->url().isLocalFile()) {
        Q_EMIT failed(tr("A terminal can only be opened in a folder on this computer."));
        return;
    }
    auto *job = new KTerminalLauncherJob(QString());
    job->setWorkingDirectory(m_folder->url().toLocalFile());
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    job->start();
}

void FileActions::contextMenu(const QList<QUrl> &urls)
{
    auto *menu = new QMenu;
    menu->setAttribute(Qt::WA_DeleteOnClose);
    const bool writable = m_folder && m_folder->canWrite();
    auto add = [&](const QString &icon, const QString &text, auto fn, bool enabled = true) {
        QAction *a = menu->addAction(QIcon::fromTheme(icon), text, this, fn);
        a->setEnabled(enabled);
        return a;
    };
    if (!urls.isEmpty()) {
        KFileItemList items;
        // The type by name only: an item whose type isn't known yet would
        // otherwise read its contents on the GUI thread (slow on a network or
        // FUSE mount). An item no longer listed is left out.
        static const QMimeDatabase mimeDb;
        for (const QUrl &u : urls.mid(0, 64)) {
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
        if (items.isEmpty()) {
            delete menu;
            return;
        }
        const bool single = urls.size() == 1;
        add(QStringLiteral("document-open"), tr("Open"), [this, urls, items, single] {
            if (single && items.first().isDir()) {
                Q_EMIT navigateRequested(urls.first());
            } else {
                openUrls(urls);
            }
        });
        if (m_folder && m_folder->searching()) {
            QList<QUrl> files;
            for (const KFileItem &it : items) {
                files << it.url();
            }
            add(QStringLiteral("folder-open"), tr("Open File Location\tCtrl+Enter"), [this, files] { Q_EMIT openLocationRequested(files); });
        }
        QList<QUrl> folders;
        for (const KFileItem &it : items) {
            if (it.isDir()) {
                folders << it.url();
            }
        }
        if (!folders.isEmpty()) {
            add(QStringLiteral("tab-new"), folders.size() == 1 ? tr("Open in New Tab") : tr("Open in New Tabs"), [this, folders] { Q_EMIT openInNewTabRequested(folders); });
        }
        if (single && folders.size() == 1) {
            const QUrl folder = folders.first();
            add(QStringLiteral("bookmark-new"), tr("Pin to Sidebar"), [folder] { PlacesLogic::instance()->pinFolder(folder); }, rustPlacesPinnable(folder.scheme()));
        }
        auto *actions = new KFileItemActions(menu);
        actions->setItemListProperties(KFileItemListProperties(items));
        actions->insertOpenWithActionsTo(nullptr, menu, {});
        actions->addActionsTo(menu, KFileItemActions::MenuActionSource::Services);
        menu->addSeparator();
        add(QStringLiteral("edit-cut"), tr("Cut"), [this, urls] { copy(urls, true); }, writable);
        add(QStringLiteral("edit-copy"), tr("Copy"), [this, urls] { copy(urls, false); });
        add(QStringLiteral("edit-rename"), tr("Rename"), [this, urls] { rename(urls.first()); }, single && writable);
        menu->addSeparator();
        add(QStringLiteral("user-trash"), tr("Move to Trash"), [this, urls] { trash(urls); }, writable);
        add(QStringLiteral("edit-delete"), tr("Delete"), [this, urls] { deleteForGood(urls); }, writable);
        menu->addSeparator();
        add(QStringLiteral("document-properties"), tr("Properties"), [this, urls] { showProperties(urls); });
    } else {
        // Search results are from many folders: nothing is made or pasted "here".
        const bool here = writable && !(m_folder && m_folder->searching());
        add(QStringLiteral("folder-new"), tr("New Folder"), [this] { newFolder(); }, here);
        add(QStringLiteral("edit-paste"), tr("Paste"), [this] { paste(); }, here && canPaste());
        menu->addSeparator();
        // What Undo and Redo would do, by name; "&" is a mnemonic in a menu.
        auto named = [](const QString &verb, const QString &what) { return what.isEmpty() ? verb : verb + QLatin1Char(' ') + QString(what).replace(QLatin1Char('&'), QStringLiteral("&&")); };
        add(QStringLiteral("edit-undo"), named(tr("Undo"), m_ops->undoText()), [this] { undo(); }, m_ops->canUndo());
        add(QStringLiteral("edit-redo"), named(tr("Redo"), m_ops->redoText()), [this] { redo(); }, m_ops->canRedo());
        menu->addSeparator();
        add(QStringLiteral("utilities-terminal"), tr("Open Terminal Here"), [this] { openTerminal(); });
        if (m_folder) {
            const QUrl here = m_folder->url();
            add(QStringLiteral("bookmark-new"), tr("Pin This Folder to Sidebar"), [here] { PlacesLogic::instance()->pinFolder(here); }, rustPlacesPinnable(here.scheme()));
        }
        QAction *hidden = add(QStringLiteral("view-visible"), tr("Show Hidden Files"), [this] {
            if (m_folder) {
                m_folder->setShowHidden(!m_folder->showHidden());
                saveShowHidden(m_folder->showHidden());
            }
        });
        hidden->setCheckable(true);
        hidden->setChecked(m_folder && m_folder->showHidden());
        menu->addSeparator();
        add(QStringLiteral("document-properties"), tr("Properties"), [this] { showProperties({}); });
    }
    menu->popup(QCursor::pos());
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
