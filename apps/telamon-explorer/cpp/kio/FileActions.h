// Everything that changes or opens files. Changes (copy, move, link, trash,
// delete, rename, new folder or file, hide, undo, redo) are queued on the
// OperationQueue, which runs them as KIO jobs; opening goes through KIO's
// prompts (Run or open?, Properties). It also decides what the context menus
// hold (`itemMenu`, `backgroundMenu`): a snapshot made before the menu is
// shown, from KFileItemActions and the core's menu rules. QML calls these;
// nothing here runs a file without KIO's prompts.
#pragma once

#include "FolderModel.h"
#include "OperationQueue.h"

#include <KFileItem>

#include <QClipboard>
#include <QFileSystemWatcher>
#include <QHash>
#include <QPointer>
#include <QQmlEngine>
#include <QQuickWindow>

#include <functional>

class KJob;
class QMenu;
class QAction;

class FileActions : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(FolderModel *folder READ folder WRITE setFolder NOTIFY folderChanged)
    Q_PROPERTY(QQuickWindow *window READ window WRITE setWindow NOTIFY windowChanged)
    Q_PROPERTY(bool canPaste READ canPaste NOTIFY canPasteChanged)
    // Every change to files runs here; QML shows its list, ring and questions.
    Q_PROPERTY(OperationQueue *operations READ operations CONSTANT)
    // How many items are cut and waiting to be pasted (moved).
    Q_PROPERTY(int cutCount READ cutCount NOTIFY cutChanged)

public:
    explicit FileActions(QObject *parent = nullptr);
    ~FileActions() override;

    FolderModel *folder() const { return m_folder; }
    void setFolder(FolderModel *f);
    QQuickWindow *window() const { return m_window; }
    void setWindow(QQuickWindow *w);
    bool canPaste() const;
    OperationQueue *operations() const { return m_ops; }
    int cutCount() const { return m_cutCount; }

    Q_INVOKABLE void openUrls(const QList<QUrl> &urls);
    Q_INVOKABLE void copy(const QList<QUrl> &urls, bool cut);
    Q_INVOKABLE void paste(const QUrl &destination = {});
    Q_INVOKABLE void trash(const QList<QUrl> &urls);
    Q_INVOKABLE void deleteForGood(const QList<QUrl> &urls);
    // Asks for the new name (`namePromptRequested`), then `acceptName` renames.
    Q_INVOKABLE void rename(const QUrl &url);
    // Asks for a name and makes the folder in the folder shown.
    Q_INVOKABLE void newFolder();
    // Asks for a name and makes a file in the folder shown: empty, or a copy
    // of `templateFile`.
    Q_INVOKABLE void newFile(const QUrl &templateFile = {});
    // Whether `name` can be used for what `request` (from namePromptRequested)
    // asks: {ok, text}; with ok, a text means a warning to confirm.
    Q_INVOKABLE QVariantMap checkName(const QString &name, const QVariantMap &request) const;
    // The prompt was confirmed: carries out the request with `name`.
    Q_INVOKABLE void acceptName(const QVariantMap &request, const QString &name);
    // Asks first ("Delete 3 items for good? This can't be undone."): the
    // window shows `deleteRequested`, and `confirmDelete` does it.
    Q_INVOKABLE void confirmDelete(const QList<QUrl> &urls);
    Q_INVOKABLE void emptyTrash();
    Q_INVOKABLE void undo();
    Q_INVOKABLE void redo();
    // What the context menu of `urls` (of the background when empty) holds,
    // made now and not changed again: {state: {key: enabled}, openWith,
    // services: the entries of KFileItemActions, ...}; see the .cpp.
    Q_INVOKABLE QVariantMap itemMenu(const QList<QUrl> &urls);
    Q_INVOKABLE QVariantMap backgroundMenu();
    // Runs the "Open With" or service-menu entry `id` of the latest menu.
    Q_INVOKABLE void runMenuAction(int id);
    // Enter on `urls`: a single folder is entered, anything else is opened.
    Q_INVOKABLE void openItems(const QList<QUrl> &urls);
    Q_INVOKABLE void openInNewTabs(const QList<QUrl> &urls);
    Q_INVOKABLE void openLocation(const QList<QUrl> &urls);
    // The full paths (the URL for a server's file), one a line, as plain text.
    Q_INVOKABLE void copyPath(const QList<QUrl> &urls);
    // Lists the items in their folder's `.hidden` file, or takes them out.
    Q_INVOKABLE void setHidden(const QList<QUrl> &urls, bool hide);
    // Compress in Telamon Archive's dialog.
    Q_INVOKABLE void compress(const QList<QUrl> &urls);
    Q_INVOKABLE void showProperties(const QList<QUrl> &urls);
    // A terminal in `folder` (the folder shown when empty).
    Q_INVOKABLE void openTerminal(const QUrl &folder = {});
    Q_INVOKABLE void startDrag(const QList<QUrl> &urls);
    // A drop of `urls` on `destination`: Shift moves, Ctrl copies, Ctrl+Shift
    // links; with no key the window shows its Move, Copy, Link menu
    // (`dropMenuRequested`) and `dropWith` carries out the choice.
    Q_INVOKABLE void drop(const QList<QUrl> &urls, const QUrl &destination);
    Q_INVOKABLE void dropWith(const QList<QUrl> &urls, const QUrl &destination, const QString &action);
    // A drop of `urls` on a folder of the path bar or the sidebar: moved
    // there, or copied when `copy`, with no menu (a name that is taken opens
    // the conflict dialog). Queued like a paste.
    Q_INVOKABLE void dropTo(const QList<QUrl> &urls, const QUrl &destination, bool copy);
    // Whether Ctrl is held right now, asked of the system (during a drag the
    // application's own record of the keys is not updated).
    Q_INVOKABLE bool copyKeyHeld() const;
    // Typed address text through the core: {ok, text} with the URL, or the reason in plain words.
    Q_INVOKABLE QVariantMap parseAddress(const QString &text) const;
    Q_INVOKABLE bool savedShowHidden() const;
    Q_INVOKABLE void saveShowHidden(bool on);

Q_SIGNALS:
    void folderChanged();
    void windowChanged();
    void canPasteChanged();
    // For the window to show in plain words.
    void failed(const QString &text);
    void navigateRequested(const QUrl &target);
    // "Open in New Tab" in the context menu: the folders chosen.
    void openInNewTabRequested(const QList<QUrl> &folders);
    // "Open File Location" on search results: the files chosen.
    void openLocationRequested(const QList<QUrl> &files);
    // The name of a rename, new folder or new file is wanted: {mode, title,
    // label, initial, okText, url, dir, isDir}. The window asks, `acceptName`
    // answers.
    void namePromptRequested(const QVariantMap &request);
    // A job started here has ended (done, failed or cancelled).
    void jobFinished();
    void cutChanged();
    // Delete for good was asked: the window asks the user, then confirmDelete.
    void deleteRequested(const QList<QUrl> &urls, const QString &text);
    // A drop without a modifier key: show Move Here, Copy Here, Link Here at x, y.
    void dropMenuRequested(const QList<QUrl> &urls, const QUrl &destination, int x, int y);

private:
    struct Template {
        QString label;
        QUrl file;
    };

    void clipboardChanged();
    KFileItemList itemsOf(const QList<QUrl> &urls) const;
    // The entries of `menu` for the window, at most two levels deep; each
    // action gets an id for `runMenuAction`.
    QVariantList entriesOf(QMenu *menu, int depth = 0);
    void dropScratch();
    QUrl terminalFolder(const KFileItemList &items) const;
    bool archiveInstalled() const;
    void loadTemplates();
    QString suggestName(const QUrl &folder, const QString &wanted) const;
    void prompt(const QVariantMap &request);

    QPointer<FolderModel> m_folder;
    QPointer<QQuickWindow> m_window;
    OperationQueue *m_ops = nullptr;
    int m_cutCount = 0;
    // What the latest menu's "Open With" and service entries run; replaced
    // (and the old ones deleted) when the next menu is made.
    QMenu *m_scratch = nullptr;
    QHash<int, QPointer<QAction>> m_menuActions;
    int m_nextActionId = 1;
    // The files of the Templates folder, read on a worker and again when the
    // folder changes, so the menu never waits for the disk.
    QList<Template> m_templates;
    QFileSystemWatcher m_templateWatch;
    int m_templateSerial = 0;
};
