// The operation queue: every copy, move, link, trash, delete, rename and new
// folder, and every undo and redo, runs here as KIO jobs paced by the core's
// state machine (atlas_explorer_core::queue, through src/ops_ffi.rs). It is a
// list model for the popover, and it asks the user (name conflicts, problems,
// KIO's "delete instead?") through `question`, which a QML dialog answers.
// See docs/DESIGN.md, "Operations".
#pragma once

#include <KIO/Global>
#include <KIO/AskUserActionInterface>

#include <QAbstractListModel>
#include <QHash>
#include <QPointer>
#include <QQmlEngine>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>

#include <functional>

class KJob;
class OperationAsker;
class QMimeData;
namespace KIO
{
class Job;
}

class OperationQueue : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("Made by FileActions")
    // Something is waiting, running, paused or asking.
    Q_PROPERTY(bool active READ active NOTIFY summaryChanged)
    // At least one operation is listed (running or finished): the ring shows.
    Q_PROPERTY(bool anyListed READ anyListed NOTIFY summaryChanged)
    // 0..1 over the running operations; -1 while nothing is measurable.
    Q_PROPERTY(double progress READ progress NOTIFY summaryChanged)
    // Every running operation is paused.
    Q_PROPERTY(bool allPaused READ allPaused NOTIFY summaryChanged)
    // The latest finished operation failed (shown on the ring).
    Q_PROPERTY(bool hasFailed READ hasFailed NOTIFY summaryChanged)
    // One line for the ring's tooltip.
    Q_PROPERTY(QString summary READ summary NOTIFY summaryChanged)
    Q_PROPERTY(bool canUndo READ canUndo NOTIFY historyChanged)
    Q_PROPERTY(bool canRedo READ canRedo NOTIFY historyChanged)
    // What the next Undo (Redo) would do, e.g. "Move 3 Items to Backup".
    Q_PROPERTY(QString undoText READ undoText NOTIFY historyChanged)
    Q_PROPERTY(QString redoText READ redoText NOTIFY historyChanged)
    // The id of the operation a finished row's Undo button belongs to (0: none).
    Q_PROPERTY(quint64 undoableId READ undoableId NOTIFY historyChanged)
    // The question being asked, {} when none. {type: "conflict" | "problem"
    // | "delete", ...}; see askConflict, askProblem, askDelete.
    Q_PROPERTY(QVariantMap question READ question NOTIFY questionChanged)

public:
    // Numbers as in the Rust core (src/ops_ffi.rs).
    enum Kind { Copy, Move, Link, Trash, Delete, Rename, NewFolder, Restore, EmptyTrash, External };
    Q_ENUM(Kind)

    enum Roles {
        IdRole = Qt::UserRole + 1,
        LabelRole,
        StateRole,
        ProgressRole,
        DetailRole,
        ErrorRole,
        CanPauseRole,
        CanResumeRole,
        CanCancelRole,
        CanRunNowRole,
        FinishedRole,
        UndoableRole,
    };

    explicit OperationQueue(QObject *parent = nullptr);
    ~OperationQueue() override;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    bool active() const;
    bool anyListed() const { return !m_ids.isEmpty(); }
    double progress() const;
    bool allPaused() const;
    bool hasFailed() const;
    QString summary() const;
    bool canUndo() const { return !m_undoTitles.isEmpty(); }
    bool canRedo() const { return !m_redoTitles.isEmpty(); }
    QString undoText() const { return m_undoTitles.value(0); }
    QString redoText() const { return m_redoTitles.value(0); }
    // The next undo and redo titles, newest first (for a menu).
    QStringList undoTitles() const { return m_undoTitles; }
    QStringList redoTitles() const { return m_redoTitles; }
    quint64 undoableId() const { return m_undoableId; }
    QVariantMap question() const { return m_questions.isEmpty() ? QVariantMap() : m_questions.first().data; }

    // What the app asks of the queue. Each returns at once; the work is
    // queued. `done(true)` runs when it finished and did what was asked.
    void transfer(Kind kind, const QList<QUrl> &sources, const QUrl &destination, std::function<void(bool)> done = {});
    void trash(const QList<QUrl> &urls);
    void deleteForGood(const QList<QUrl> &urls);
    void rename(const QUrl &url, const QString &newName);
    void makeFolder(const QUrl &folder, const QString &name);
    // A new file named `name` in `folder`: empty, or a copy of `templateFile`.
    // Undone like a copy (the new file goes to the Trash).
    void makeFile(const QUrl &folder, const QString &name, const QUrl &templateFile = {});
    // Lists the items (all in one folder on this computer) in that folder's
    // `.hidden` file, or takes them out of it. Not undoable.
    void setHidden(const QList<QUrl> &urls, bool hide, std::function<void(bool)> done = {});
    // Pastes text or an image from the clipboard as a file (KIO asks for the name).
    void pasteData(const QMimeData *data, const QUrl &destination);
    void emptyTrash();

    Q_INVOKABLE void undo();
    Q_INVOKABLE void redo();
    Q_INVOKABLE void pause(quint64 id);
    Q_INVOKABLE void resume(quint64 id);
    Q_INVOKABLE void cancel(quint64 id);
    Q_INVOKABLE void runNow(quint64 id);
    // Takes a finished operation out of the list (0: all finished ones).
    Q_INVOKABLE void dismiss(quint64 id);
    // Answers the question being asked: {answer: "replace" | "skip" |
    // "keepBoth" | "merge" | "retry" | "skipAll" | "yes" | "cancel", all: bool,
    // name: the new name for keepBoth}.
    Q_INVOKABLE void answer(const QVariantMap &reply);
    // Whether `name` is a good name for the Keep Both copy: "" or the reason.
    Q_INVOKABLE QString checkName(const QString &name, const QString &existingName) const;

    // Called by OperationAsker for the job of operation `opId`.
    void askConflict(OperationAsker *asker, quint64 opId, KJob *job, const QUrl &src, const QUrl &dest, KIO::RenameDialog_Options options,
                     KIO::filesize_t sizeSrc, KIO::filesize_t sizeDest, const QDateTime &mtimeSrc, const QDateTime &mtimeDest,
                     const QDateTime &ctimeSrc, const QDateTime &ctimeDest);
    void askProblem(OperationAsker *asker, quint64 opId, KJob *job, KIO::SkipDialog_Options options, const QString &text);
    void askDelete(OperationAsker *asker, quint64 opId, const QList<QUrl> &urls, KIO::AskUserActionInterface::DeletionType type);

Q_SIGNALS:
    void summaryChanged();
    void historyChanged();
    void questionChanged();
    // For the window to say in plain words (a toast).
    void message(const QString &text);
    // A copy or move that was refused before it started: a title and the reason.
    void refused(const QString &title, const QString &text);
    // An operation ended (done, failed or cancelled).
    void jobFinished();

private:
    struct Work;
    struct Question {
        quint64 opId = 0;
        QVariantMap data;
        std::function<void(const QVariantMap &)> reply;
    };

    using HistoryJob = std::function<void(std::function<void()> next)>;

    quint64 enqueue(Work work, const QString &runningLabel);
    void pump();
    void startOp(quint64 id);
    void runStep(quint64 id);
    void attach(Work &w, KJob *job);
    void stepDone(quint64 id, KJob *job);
    void finishOp(quint64 id);
    void failOp(quint64 id, const QString &why, bool say = true);
    void endOp(quint64 id, bool ok = false);
    void refresh();
    void refreshRows();
    void refreshHistory();
    void tick();
    int rowOf(quint64 id) const;
    void removeQuestions(quint64 id);
    void recordHistory(const Work &w);
    void completeHistory(const Work &w);
    void startHistory(int side);
    void histEnqueue(HistoryJob job);
    void histPump();
    void runHistory(int side, std::function<void()> next);
    QString nameList(const QList<QUrl> &urls, int max) const;
    static QString folderName(const QUrl &folder);
    static QString key(const QUrl &url);
    QString textFor(uint32_t which, Kind kind, const QList<QUrl> &urls, const QString &to, const QString &newName) const;
    Work &work(quint64 id);

    void *m_engine = nullptr;
    QList<quint64> m_ids;
    QHash<quint64, Work> m_work;
    QList<Question> m_questions;
    QStringList m_undoTitles;
    QStringList m_redoTitles;
    quint64 m_undoableId = 0;
    // Everything that reads or changes the undo lists (writing down a
    // finished operation, an undo, a redo) runs one after the other, in the
    // order it was asked: each needs the files looked at first, and each must
    // find the lists as the one before left them. A job calls `next` when done.
    QList<HistoryJob> m_histJobs;
    bool m_histRunning = false;
    QTimer m_tick;
};
