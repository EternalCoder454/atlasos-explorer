// Everything that changes or opens files. Changes (copy, move, link, trash,
// delete, rename, new folder or file, hide, undo, redo) are queued on the
// OperationQueue, which runs them as KIO jobs; opening goes through KIO's
// prompts (Run or open?, Properties). It also decides what the context menus
// hold (`itemMenu`, `backgroundMenu`): a snapshot made before the menu is
// shown, from KFileItemActions and the core's menu rules. QML calls these;
// nothing here runs a file without KIO's prompts.
#pragma once

#include "FolderModel.h"
#include "ImageWork.h"
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
    // F2, Rename: asks the window to rename `urls` (`renameRequested`); it
    // renames one item in place and several in the Batch Rename dialog.
    Q_INVOKABLE void rename(const QList<QUrl> &urls);
    // Asks for one item's new name in a dialog (`namePromptRequested`), for
    // where a name can't be edited in place; `acceptName` renames.
    Q_INVOKABLE void renameWithDialog(const QUrl &url);
    // Renames `url` to `name` (checked again here), through the queue. With
    // `select`, the renamed item is selected and shown when it is done.
    Q_INVOKABLE void renameTo(const QUrl &url, const QString &name, bool select);
    // Makes a folder in the folder shown, named "New Folder" (or the first
    // free "New Folder (2)"...), and says `createdItem` when it is there.
    Q_INVOKABLE void newFolder();
    // The same for a file: empty, or a copy of `templateFile`.
    Q_INVOKABLE void newFile(const QUrl &templateFile = {});
    // Whether `name` can be used for what `request` (from namePromptRequested,
    // or {mode: "rename", url}) asks: {ok, text}; with ok, a text means a
    // warning to confirm.
    Q_INVOKABLE QVariantMap checkName(const QString &name, const QVariantMap &request) const;
    // The prompt was confirmed: carries out the request with `name`.
    Q_INVOKABLE void acceptName(const QVariantMap &request, const QString &name);
    // The real name of `url` (not the display form), and how many of its
    // characters to select first when it is edited: {name, stem}.
    Q_INVOKABLE QVariantMap editableName(const QUrl &url, bool isDir) const;
    // Says `text` in the window's line for problems.
    Q_INVOKABLE void tell(const QString &text);
    // Batch Rename. `spec`: {mode: "replace" | "number" | "case" | "text",
    // find, replace, matchCase, regex, start, step, padding, atEnd, separator,
    // text, caseMode: "lower" | "upper" | "title" | "sentence"}. The preview is
    // {valid, canApply, changed, blocked, problem, rows: [{old, new, code,
    // text}]} in the order of the folder shown (the names are display names;
    // code: 0 unchanged, 1 fine, 2 warning, 3 not a name, 4 same as another,
    // 5 a selected item has it, 6 in the folder). Apply plans again, and does
    // nothing unless the plan can be applied.
    Q_INVOKABLE QVariantMap batchPreview(const QList<QUrl> &urls, const QVariantMap &spec) const;
    Q_INVOKABLE void batchApply(const QList<QUrl> &urls, const QVariantMap &spec);
    // Asks first ("Delete 3 items for good? This can't be undone."): the
    // window shows `deleteRequested`, and `confirmDelete` does it.
    Q_INVOKABLE void confirmDelete(const QList<QUrl> &urls);
    Q_INVOKABLE void emptyTrash();
    // Puts items of the Trash (its top) back where they were. A folder that
    // is gone is made again only after the user says so (`restoreAsk`, then
    // `confirmRestore` or `cancelRestore`); an original place that is taken
    // opens the conflict dialog.
    Q_INVOKABLE void restore(const QList<QUrl> &urls);
    Q_INVOKABLE void confirmRestore();
    Q_INVOKABLE void cancelRestore();
    // Removes what has been in the Trash for more than `days` days (quiet;
    // the Trash setting). Not undoable.
    Q_INVOKABLE void emptyOldTrash(int days);
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
    // Telamon Archive's jobs, shown in the operation queue: extract the
    // archives next to where they are, or make a ZIP of the files.
    Q_INVOKABLE void extractHere(const QList<QUrl> &urls);
    Q_INVOKABLE void compressToZip(const QList<QUrl> &urls);
    // Extract in Telamon Archive's dialog (it asks where).
    Q_INVOKABLE void extractTo(const QList<QUrl> &urls);
    // The Extract button of an archive opened as a folder: everything in it,
    // by Telamon Archive when it is installed, else by Files into a new
    // folder named after the archive.
    Q_INVOKABLE void extractViewed();
    // Asks the window for the Properties window of `urls` (of the folder
    // shown when empty): `propertiesRequested`.
    Q_INVOKABLE void showProperties(const QList<QUrl> &urls);
    // Tags, through the operation queue (undoable). `on`: put the tag on every
    // item that lacks it; else take it off every item that has it.
    Q_INVOKABLE void toggleTag(const QList<QUrl> &urls, const QString &name, bool on);
    Q_INVOKABLE void clearTags(const QList<QUrl> &urls);
    // Asks for the name of a new tag (`namePromptRequested`, mode "tag"), then puts it on `urls`.
    Q_INVOKABLE void newTag(const QList<QUrl> &urls);
    // The star rating, 0 to 10 (two to a star; 0 takes it away).
    Q_INVOKABLE void setRating(const QList<QUrl> &urls, int rating);
    // Permissions: bits of 0777 to turn on and off; `recursive` also changes
    // what is inside folders (files keep their run bits).
    Q_INVOKABLE void setPermissions(const QList<QUrl> &urls, uint setBits, uint clearBits, bool recursive);
    // Quick actions on pictures (More Actions): 0 Rotate Left, 1 Rotate Right,
    // 2 Convert to PNG, 3 to JPEG, 4 to WebP, 5 Combine into PDF (pictures and
    // PDFs, in the order of `urls`). They make new files beside the originals
    // (a worker; the copy into place goes through the queue, so a taken name
    // asks, and Undo takes the new files to the Trash). Says in a line why
    // when it can't.
    Q_INVOKABLE void pictureAction(const QList<QUrl> &urls, int action);
    // Copies or moves `urls` into `folder` (the other pane's), through the
    // queue like a drop on a path bar segment; says so when it would change
    // nothing.
    Q_INVOKABLE void transferTo(const QList<QUrl> &urls, const QUrl &folder, bool move);
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
    // The name of a rename is wanted in a dialog: {mode, title, label,
    // initial, okText, url, isDir}. The window asks, `acceptName` answers.
    void namePromptRequested(const QVariantMap &request);
    // Rename these items (the window picks in place or Batch Rename).
    void renameRequested(const QList<QUrl> &urls);
    // A new folder or file is there (in the folder shown): the window shows it
    // and puts its name in edit mode.
    void createdItem(const QUrl &url);
    // A job started here has ended (done, failed or cancelled).
    void jobFinished();
    // Files an extraction or compression made: the window selects them.
    void resultsReady(const QList<QUrl> &urls);
    void cutChanged();
    // Delete for good was asked: the window asks the user, then confirmDelete.
    void deleteRequested(const QList<QUrl> &urls, const QString &text);
    // Restore found folders that are gone: the window asks (title, text), and
    // `confirmRestore` or `cancelRestore` answers.
    void restoreAsk(const QString &title, const QString &text);
    // Show the Properties window for these items.
    void propertiesRequested(const QList<QUrl> &urls);
    // A drop without a modifier key: show Move Here, Copy Here, Link Here at x, y.
    void dropMenuRequested(const QList<QUrl> &urls, const QUrl &destination, int x, int y);

private:
    struct Template {
        QString label;
        QUrl file;
    };

    void clipboardChanged();
    KFileItemList itemsOf(const QList<QUrl> &urls) const;
    QList<QUrl> inFolderOrder(const QList<QUrl> &urls) const;
    // The entries of `menu` for the window, at most two levels deep; each
    // action gets an id for `runMenuAction`.
    QVariantList entriesOf(QMenu *menu, int depth = 0);
    void dropScratch();
    QUrl terminalFolder(const KFileItemList &items) const;
    bool archiveInstalled() const;
    // The MIME types Telamon Archive's desktop file says it opens.
    QStringList archiveMimeTypes() const;
    // Where a double click on an archive file goes: its contents as a folder
    // (`zip:/...`); invalid when it isn't one KIO's archive worker opens.
    QUrl browseUrl(const QUrl &url) const;
    // Calls Archive for a job that is shown in the queue.
    void runArchive(const QString &method, const QList<QUrl> &urls, const QVariantList &extra, const QString &title, const QString &running);
    // The options every call to Archive carries; the activation token comes
    // asynchronously on Wayland.
    void withArchiveOptions(bool showProgress, std::function<void(QVariantMap)> go);
    QString labelOf(const QList<QUrl> &urls) const;
    // The Tags submenu of the context menu, from what is known of the items now.
    QVariantMap tagMenu(const QList<QUrl> &urls) const;
    // Which picture actions `urls` take: {rotate, png, jpeg, webp, combine}.
    QVariantMap pictureMenu(const QList<QUrl> &urls) const;
    struct PictureSet {
        bool ok = false;
        QString why;
        QUrl folder;
        QList<ImageWork::Source> sources;
        QStringList names;
    };
    // The files an action would work on, in the order given, or why not.
    PictureSet pictureSet(const QList<QUrl> &urls, int action) const;
    void loadTemplates();
    QString suggestName(const QUrl &folder, const QString &wanted) const;

    QPointer<FolderModel> m_folder;
    QPointer<QQuickWindow> m_window;
    OperationQueue *m_ops = nullptr;
    int m_cutCount = 0;
    // A restore that waits for the answer about folders that are gone.
    QList<OperationQueue::RestoreItem> m_restoreItems;
    QStringList m_restoreFolders;
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
