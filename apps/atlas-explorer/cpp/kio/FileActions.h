// Everything that changes or opens files, as KIO jobs with KIO's own dialogs
// (Run or open?, conflicts, delete confirmation, Properties) and Plasma's job
// tracker. QML calls these; nothing here runs a file without KIO's prompts.
#pragma once

#include "FolderModel.h"

#include <QClipboard>
#include <QPointer>
#include <QQmlEngine>
#include <QQuickWindow>

#include <functional>

class KJob;
namespace KIO
{
class WidgetsAskUserActionHandler;
}

class FileActions : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(FolderModel *folder READ folder WRITE setFolder NOTIFY folderChanged)
    Q_PROPERTY(QQuickWindow *window READ window WRITE setWindow NOTIFY windowChanged)
    Q_PROPERTY(bool canPaste READ canPaste NOTIFY canPasteChanged)

public:
    explicit FileActions(QObject *parent = nullptr);

    FolderModel *folder() const { return m_folder; }
    void setFolder(FolderModel *f);
    QQuickWindow *window() const { return m_window; }
    void setWindow(QQuickWindow *w);
    bool canPaste() const;

    Q_INVOKABLE void openUrls(const QList<QUrl> &urls);
    Q_INVOKABLE void copy(const QList<QUrl> &urls, bool cut);
    Q_INVOKABLE void paste(const QUrl &destination = {});
    Q_INVOKABLE void trash(const QList<QUrl> &urls);
    Q_INVOKABLE void deleteForGood(const QList<QUrl> &urls);
    Q_INVOKABLE void rename(const QUrl &url);
    Q_INVOKABLE void newFolder();
    Q_INVOKABLE void undo();
    Q_INVOKABLE void contextMenu(const QList<QUrl> &urls);
    Q_INVOKABLE void showProperties(const QList<QUrl> &urls);
    Q_INVOKABLE void openTerminal();
    Q_INVOKABLE void startDrag(const QList<QUrl> &urls);
    // A drop of `urls` on `destination`, with KIO's copy/move/link menu.
    Q_INVOKABLE void drop(const QList<QUrl> &urls, const QUrl &destination);
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

private:
    void setup(KJob *job);
    void askName(const QString &title, const QString &label, const QString &initial, std::function<void(const QString &)> done);
    void tune(QWidget *dialog);

    QPointer<FolderModel> m_folder;
    QPointer<QQuickWindow> m_window;
    KIO::WidgetsAskUserActionHandler *m_ask = nullptr;
};
