#include "FileActions.h"

#include "RustBridge.h"

#include <KConfigGroup>
#include <KFileItemActions>
#include <KFileItemListProperties>
#include <KIO/CopyJob>
#include <KIO/DeleteJob>
#include <KIO/DropJob>
#include <KIO/FileUndoManager>
#include <KIO/Job>
#include <KIO/JobTracker>
#include <KIO/JobUiDelegateFactory>
#include <KIO/MkdirJob>
#include <KIO/OpenUrlJob>
#include <KIO/Paste>
#include <KIO/PasteJob>
#include <KIO/SimpleJob>
#include <KIO/WidgetsAskUserActionHandler>
#include <KJobTrackerInterface>
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
}

FileActions::FileActions(QObject *parent)
    : QObject(parent)
{
    m_ask = new KIO::WidgetsAskUserActionHandler(this);
    connect(QApplication::clipboard(), &QClipboard::dataChanged, this, &FileActions::canPasteChanged);
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

// Every job gets KIO's delegate (conflict and error dialogs), its window, and
// Plasma's job tracker (progress, pause, cancel).
void FileActions::setup(KJob *job)
{
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    if (m_window) {
        KJobWindows::setWindow(job, m_window);
    }
    KIO::getJobTracker()->registerJob(job);
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
        KIO::CopyJob *job = cut ? KIO::move(urls, dest) : KIO::copy(urls, dest);
        setup(job);
        KIO::FileUndoManager::self()->recordCopyJob(job);
        if (cut) {
            // The files are moving: a second paste would find them gone.
            connect(job, &KJob::result, this, [](KJob *j) {
                if (!j->error()) {
                    QApplication::clipboard()->clear();
                }
            });
        }
        return;
    }
    // Text or an image: KIO asks for a file name and writes it.
    if (KIO::PasteJob *job = KIO::paste(md, dest)) {
        setup(job);
    }
}

void FileActions::trash(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    KIO::Job *job = KIO::trash(urls);
    setup(job);
    KIO::FileUndoManager::self()->recordJob(KIO::FileUndoManager::Trash, urls, QUrl(QStringLiteral("trash:/")), job);
}

void FileActions::deleteForGood(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    // One-shot: the answer comes back through the handler's signal.
    auto *conn = new QMetaObject::Connection;
    *conn = connect(m_ask, &KIO::AskUserActionInterface::askUserDeleteResult, this, [this, conn, urls](bool allow, const QList<QUrl> &asked, auto, QWidget *) {
        if (asked != urls) {
            return;
        }
        disconnect(*conn);
        delete conn;
        if (allow) {
            setup(KIO::del(urls));
        }
    });
    m_ask->askUserDelete(urls, KIO::AskUserActionInterface::Delete, KIO::AskUserActionInterface::ForceConfirmation, nullptr);
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
        const QUrl target = childUrl(url.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash), name);
        // A move job, not KIO::rename: the undo manager records only copy jobs.
        KIO::CopyJob *job = KIO::moveAs(url, target, KIO::HideProgressInfo);
        setup(job);
        KIO::FileUndoManager::self()->recordJob(KIO::FileUndoManager::Rename, {url}, target, job);
    });
}

void FileActions::newFolder()
{
    if (!m_folder) {
        return;
    }
    const QUrl dir = m_folder->url();
    askName(tr("New Folder"), tr("Folder name:"), tr("New Folder"), [this, dir](const QString &name) {
        const QUrl target = childUrl(dir, name);
        KIO::SimpleJob *job = KIO::mkdir(target);
        setup(job);
        KIO::FileUndoManager::self()->recordJob(KIO::FileUndoManager::Mkdir, {}, target, job);
    });
}

void FileActions::undo()
{
    // v0.1 uses KIO's undo manager; the core's own record comes later.
    auto *um = KIO::FileUndoManager::self();
    if (um->isUndoAvailable()) {
        um->undo();
    }
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
        QList<QUrl> folders;
        for (const KFileItem &it : items) {
            if (it.isDir()) {
                folders << it.url();
            }
        }
        if (!folders.isEmpty()) {
            add(QStringLiteral("tab-new"), folders.size() == 1 ? tr("Open in New Tab") : tr("Open in New Tabs"), [this, folders] { Q_EMIT openInNewTabRequested(folders); });
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
        add(QStringLiteral("folder-new"), tr("New Folder"), [this] { newFolder(); }, writable);
        add(QStringLiteral("edit-paste"), tr("Paste"), [this] { paste(); }, writable && canPaste());
        menu->addSeparator();
        add(QStringLiteral("utilities-terminal"), tr("Open Terminal Here"), [this] { openTerminal(); });
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
    // Nothing to do for a drop back where the items already are, or onto one
    // of them.
    bool anywhere = false;
    for (const QUrl &u : urls) {
        if (u == destination) {
            return;
        }
        if (u.adjusted(QUrl::RemoveFilename | QUrl::StripTrailingSlash) != destination.adjusted(QUrl::StripTrailingSlash)) {
            anywhere = true;
        }
    }
    if (!anywhere) {
        return;
    }
    // The drop event is rebuilt: Qt Quick's DropArea doesn't hand out its own.
    // The mime data lives as long as the job's menu does.
    auto *md = new QMimeData;
    KUrlMimeData::setUrls(urls, urls, md);
    const Qt::KeyboardModifiers mods = QGuiApplication::queryKeyboardModifiers();
    auto *event = new QDropEvent(QPointF(), Qt::CopyAction | Qt::MoveAction | Qt::LinkAction, md, Qt::LeftButton, mods);
    KIO::DropJob *job = KIO::drop(event, destination);
    if (!job) {
        delete event;
        delete md;
        return;
    }
    job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
    if (m_window) {
        KJobWindows::setWindow(job, m_window);
    }
    connect(job, &KJob::result, md, [event, md] {
        delete event;
        delete md;
    });
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
