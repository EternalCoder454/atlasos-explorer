#include "OperationAsker.h"

#include "OperationQueue.h"

OperationAsker::OperationAsker(OperationQueue *queue, quint64 opId, QObject *parent)
    : KIO::AskUserActionInterface(parent)
    , m_queue(queue)
    , m_opId(opId)
{
}

void OperationAsker::askUserRename(KJob *job,
                                   const QString &,
                                   const QUrl &src,
                                   const QUrl &dest,
                                   KIO::RenameDialog_Options options,
                                   KIO::filesize_t sizeSrc,
                                   KIO::filesize_t sizeDest,
                                   const QDateTime &ctimeSrc,
                                   const QDateTime &ctimeDest,
                                   const QDateTime &mtimeSrc,
                                   const QDateTime &mtimeDest)
{
    m_queue->askConflict(this, m_opId, job, src, dest, options, sizeSrc, sizeDest, mtimeSrc, mtimeDest, ctimeSrc, ctimeDest);
}

void OperationAsker::askUserSkip(KJob *job, KIO::SkipDialog_Options options, const QString &errorText)
{
    m_queue->askProblem(this, m_opId, job, options, errorText);
}

void OperationAsker::askUserDelete(const QList<QUrl> &urls, DeletionType deletionType, ConfirmationType, QWidget *)
{
    m_queue->askDelete(this, m_opId, urls, deletionType);
}

void OperationAsker::requestUserMessageBox(MessageDialogType, const QString &, const QString &, const QString &, const QString &, const QString &, const QString &,
                                           const QString &, const QString &, QWidget *)
{
    // KIO::WorkerBase::Cancel
    Q_EMIT messageBoxResult(2);
}

void OperationAsker::askIgnoreSslErrors(const QVariantMap &, QWidget *)
{
    Q_EMIT askIgnoreSslErrorsResult(0);
}
