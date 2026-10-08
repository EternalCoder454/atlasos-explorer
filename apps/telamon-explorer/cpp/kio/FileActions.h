// Everything that changes or opens files. Changes (copy, move, link, trash,
// delete, rename, new folder, undo, redo) are queued on the OperationQueue,
// which runs them as KIO jobs; opening goes through KIO's prompts (Run or
// open?, Properties). QML calls these; nothing here runs a file without
// KIO's prompts.
#pragma once

#include "FolderModel.h"
#include "OperationQueue.h"

#include <QClipboard>
#include <QPointer>
#include <QQmlEngine>
#include <QQuickWindow>

#include <functional>

class KJob;

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
    Q_INVOKABLE void rename(const QUrl &url);
    Q_INVOKABLE void newFolder();
    // Asks first ("Delete 3 items for good? This can't be undone."): the
    // window shows `deleteRequested`, and `confirmDelete` does it.
    Q_INVOKABLE void confirmDelete(const QList<QUrl> &urls);
    Q_INVOKABLE void emptyTrash();
    Q_INVOKABLE void undo();
    Q_INVOKABLE void redo();
    Q_INVOKABLE void contextMenu(const QList<QUrl> &urls);
    Q_INVOKABLE void showProperties(const QList<QUrl> &urls);
    Q_INVOKABLE void openTerminal();
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
    // A job started here has ended (done, failed or cancelled).
    void jobFinished();
    void cutChanged();
    // Delete for good was asked: the window asks the user, then confirmDelete.
    void deleteRequested(const QList<QUrl> &urls, const QString &text);
    // A drop without a modifier key: show Move Here, Copy Here, Link Here at x, y.
    void dropMenuRequested(const QList<QUrl> &urls, const QUrl &destination, int x, int y);

private:
    void clipboardChanged();
    void askName(const QString &title, const QString &label, const QString &initial, std::function<void(const QString &)> done);
    void tune(QWidget *dialog);

    QPointer<FolderModel> m_folder;
    QPointer<QQuickWindow> m_window;
    OperationQueue *m_ops = nullptr;
    int m_cutCount = 0;
};
