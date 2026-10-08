// What a KIO job asks the user, answered by the operation queue: the job's UI
// delegate has one of these as its child (KIO::delegateExtension finds it
// there). Conflicts, "skip this file?" problems and "delete instead of
// trash?" go to QML dialogs through OperationQueue::question; nothing here
// shows a widget.
#pragma once

#include <KIO/AskUserActionInterface>

#include <QPointer>

class OperationQueue;

class OperationAsker : public KIO::AskUserActionInterface
{
    Q_OBJECT

public:
    OperationAsker(OperationQueue *queue, quint64 opId, QObject *parent);

    quint64 opId() const { return m_opId; }

    void askUserRename(KJob *job,
                       const QString &title,
                       const QUrl &src,
                       const QUrl &dest,
                       KIO::RenameDialog_Options options,
                       KIO::filesize_t sizeSrc,
                       KIO::filesize_t sizeDest,
                       const QDateTime &ctimeSrc = {},
                       const QDateTime &ctimeDest = {},
                       const QDateTime &mtimeSrc = {},
                       const QDateTime &mtimeDest = {}) override;
    void askUserSkip(KJob *job, KIO::SkipDialog_Options options, const QString &errorText) override;
    void askUserDelete(const QList<QUrl> &urls, DeletionType deletionType, ConfirmationType confirmationType, QWidget *parent = nullptr) override;
    // Workers' own prompts (a certificate, a password-less share) are not
    // answered for the user: they are declined, and the job fails in words.
    void requestUserMessageBox(MessageDialogType type,
                               const QString &text,
                               const QString &title,
                               const QString &primaryActionText,
                               const QString &secondatyActionText,
                               const QString &primaryActionIconName = {},
                               const QString &secondatyActionIconName = {},
                               const QString &dontAskAgainName = {},
                               const QString &details = {},
                               QWidget *parent = nullptr) override;
    void askIgnoreSslErrors(const QVariantMap &sslErrorData, QWidget *parent) override;

    // The queue's answers.
    void replyRename(KIO::RenameDialog_Result result, const QUrl &newUrl, KJob *job) { Q_EMIT askUserRenameResult(result, newUrl, job); }
    void replySkip(KIO::SkipDialog_Result result, KJob *job) { Q_EMIT askUserSkipResult(result, job); }
    void replyDelete(bool allow, const QList<QUrl> &urls, DeletionType type) { Q_EMIT askUserDeleteResult(allow, urls, type, nullptr); }

private:
    OperationQueue *m_queue;
    quint64 m_opId;
};
