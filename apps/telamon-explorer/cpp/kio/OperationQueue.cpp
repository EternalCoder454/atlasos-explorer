#include "OperationQueue.h"

#include "ArchiveClient.h"
#include "ArchiveGuard.h"
#include "OperationAsker.h"
#include "OpsBridge.h"
#include "RustBridge.h"

#include <KIO/CopyJob>
#include <KIO/DeleteJob>
#include <KIO/EmptyTrashJob>
#include <KIO/FileCopyJob>
#include <KIO/Job>
#include <KIO/JobUiDelegateFactory>
#include <KIO/MkdirJob>
#include <KIO/Paste>
#include <KIO/PasteJob>
#include <KIO/RestoreJob>
#include <KIO/SimpleJob>
#include <KIO/StatJob>
#include <KIO/StoredTransferJob>
#include <KJobUiDelegate>

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QLocale>
#include <QMimeData>
#include <QMimeDatabase>
#include <QMutex>
#include <QSaveFile>
#include <QSet>
#include <QThreadPool>

#include <algorithm>
#include <memory>

#include <dirent.h>
#include <sys/stat.h>

namespace
{
// What was seen of a file, for the undo record and its check.
struct SeenState {
    bool exists = false;
    bool isDir = false;
    quint64 size = 0;
    qint64 mtime = 0;
    // -1: not counted (a server, the Trash).
    qint64 entries = -1;
};

SeenState statLocal(const QString &path)
{
    SeenState s;
    struct stat st;
    if (::lstat(QFile::encodeName(path).constData(), &st) != 0) {
        return s;
    }
    s.exists = true;
    s.isDir = S_ISDIR(st.st_mode);
    s.size = s.isDir ? 0 : quint64(st.st_size);
    s.mtime = qint64(st.st_mtim.tv_sec);
    if (s.isDir) {
        qint64 n = 0;
        if (DIR *d = ::opendir(QFile::encodeName(path).constData())) {
            while (const dirent *e = ::readdir(d)) {
                const char *nm = e->d_name;
                if (!(nm[0] == '.' && (nm[1] == 0 || (nm[1] == '.' && nm[2] == 0)))) {
                    ++n;
                }
                // A folder this big is "not empty" and "changed" either way.
                if (n >= 100000) {
                    break;
                }
            }
            ::closedir(d);
        }
        s.entries = n;
    }
    return s;
}

// How a path of a record is written: the URL, percent-encoded, no trailing slash.
QString urlKey(const QUrl &url)
{
    return url.adjusted(QUrl::StripTrailingSlash).toString(QUrl::FullyEncoded);
}

QUrl urlOfKey(const QString &key)
{
    return QUrl::fromEncoded(key.toUtf8());
}

// One line of a states text: the path, then "missing" or the four fields.
QString stateLine(const QString &key, const SeenState &s)
{
    if (!s.exists) {
        return key + QLatin1String("\tmissing\n");
    }
    return key + QLatin1Char('\t') + (s.isDir ? QLatin1Char('d') : QLatin1Char('f')) + QLatin1Char('\t') + QString::number(s.size) + QLatin1Char('\t')
        + QString::number(s.mtime) + QLatin1Char('\t') + (s.entries < 0 ? QStringLiteral("-") : QString::number(s.entries)) + QLatin1Char('\n');
}

// The fields after the path, for a record line.
QString stateFields(const SeenState &s)
{
    return QString(s.isDir ? QLatin1Char('d') : QLatin1Char('f')) + QLatin1Char('\t') + QString::number(s.size) + QLatin1Char('\t') + QString::number(s.mtime)
        + QLatin1Char('\t') + (s.entries < 0 ? QStringLiteral("-") : QString::number(s.entries));
}

using SeenMap = QHash<QString, SeenState>;

// Looks at files without blocking the window: local ones on a worker (lstat),
// the rest (a server, the Trash) through KIO stat jobs. `done` gets every URL
// asked for, by its key; one that can't be seen is missing.
class StatBatch : public QObject
{
public:
    static void run(const QList<QUrl> &urls, QObject *context, std::function<void(const SeenMap &)> done)
    {
        auto *b = new StatBatch(context);
        b->m_done = std::move(done);
        QStringList local;
        QList<QUrl> remote;
        for (const QUrl &u : urls) {
            if (u.isLocalFile()) {
                local << u.toLocalFile();
            } else {
                remote << u;
            }
        }
        b->m_pending = (local.isEmpty() ? 0 : 1) + int(remote.size());
        if (b->m_pending == 0) {
            QMetaObject::invokeMethod(b, [b] { b->finish(); }, Qt::QueuedConnection);
            return;
        }
        if (!local.isEmpty()) {
            QPointer<StatBatch> guard(b);
            QThreadPool::globalInstance()->start([guard, local] {
                SeenMap found;
                for (const QString &p : local) {
                    found.insert(urlKey(QUrl::fromLocalFile(p)), statLocal(p));
                }
                if (!guard) {
                    return;
                }
                QMetaObject::invokeMethod(guard.data(), [guard, found] {
                    if (guard) {
                        guard->m_seen.insert(found);
                        guard->one();
                    }
                });
            });
        }
        for (const QUrl &u : remote) {
            KIO::StatJob *job = KIO::stat(u, KIO::StatJob::SourceSide, KIO::StatBasic | KIO::StatTime, KIO::HideProgressInfo);
            connect(job, &KJob::result, b, [b, u, job] {
                SeenState s;
                if (!job->error()) {
                    const KIO::UDSEntry e = job->statResult();
                    s.exists = true;
                    s.isDir = e.isDir();
                    s.size = s.isDir ? 0 : quint64(e.numberValue(KIO::UDSEntry::UDS_SIZE, 0));
                    s.mtime = e.numberValue(KIO::UDSEntry::UDS_MODIFICATION_TIME, 0);
                }
                b->m_seen.insert(urlKey(u), s);
                b->one();
            });
        }
    }

private:
    explicit StatBatch(QObject *context)
        : QObject(context)
    {
    }
    void one()
    {
        if (--m_pending == 0) {
            finish();
        }
    }
    void finish()
    {
        m_done(m_seen);
        deleteLater();
    }

    SeenMap m_seen;
    std::function<void(const SeenMap &)> m_done;
    int m_pending = 0;
};

bool isTransferKind(int kind)
{
    return kind == OperationQueue::Copy || kind == OperationQueue::Move || kind == OperationQueue::Delete || kind == OperationQueue::EmptyTrash
        || kind == OperationQueue::External;
}

QString stateName(uint32_t s)
{
    static const char *const names[] = {"waiting", "running", "paused", "asking", "done", "failed", "cancelled"};
    return QLatin1String(names[qMin<uint32_t>(s, 6)]);
}

}

namespace
{
// Adds names to a folder's `.hidden` file (one name a line, which the file
// managers and the index read) or takes them out, on a worker thread. It
// starts itself, as KIO's jobs do.
class HiddenFileJob : public KJob
{
public:
    HiddenFileJob(const QString &dir, const QStringList &names, bool hide)
        : m_dir(dir)
        , m_names(names)
        , m_hide(hide)
    {
        QMetaObject::invokeMethod(this, &HiddenFileJob::start, Qt::QueuedConnection);
    }

    void start() override
    {
        QPointer<HiddenFileJob> self(this);
        const QString dir = m_dir;
        const QStringList names = m_names;
        const bool hide = m_hide;
        QThreadPool::globalInstance()->start([self, dir, names, hide] {
            const QString why = change(dir, names, hide);
            if (!self) {
                return;
            }
            QMetaObject::invokeMethod(self.data(), [self, why] {
                if (!self) {
                    return;
                }
                if (!why.isEmpty()) {
                    self->setError(KJob::UserDefinedError);
                    self->setErrorText(why);
                }
                self->emitResult();
            });
        });
    }

protected:
    bool doKill() override { return true; }

private:
    // "" when done, else why not.
    static QString change(const QString &dir, const QStringList &names, bool hide)
    {
        // Two edits of one file at once would each start from the old text and
        // the later commit would drop the earlier's change.
        static QMutex lock;
        const QMutexLocker locker(&lock);
        const QString path = dir + QStringLiteral("/.hidden");
        const QFileInfo info(path);
        // A link could lead anywhere; a file managed by hand is left alone.
        if (info.isSymLink()) {
            return tr("The .hidden file in this folder is a link, so it was not changed.");
        }
        QByteArray content;
        if (info.exists()) {
            if (!info.isFile()) {
                return tr("The .hidden file in this folder is not a plain file.");
            }
            QFile f(path);
            if (!f.open(QIODevice::ReadOnly)) {
                return tr("The .hidden file in this folder can't be read.");
            }
            content = f.read(qint64(1 << 20) + 2);
        }
        // Rewriting a file that is not text would change it.
        if (QString::fromUtf8(content).toUtf8() != content) {
            return tr("The .hidden file in this folder is not text, so it was not changed.");
        }
        const RustHidden r = rustMenuHidden(hide, content, names);
        if (r.rc == 1) {
            return {};
        }
        if (r.rc != 0) {
            return QString::fromUtf8(r.text);
        }
        QSaveFile out(path);
        if (!out.open(QIODevice::WriteOnly) || out.write(r.text) != r.text.size() || !out.commit()) {
            return tr("The .hidden file in this folder can't be written.");
        }
        return {};
    }

    QString m_dir;
    QStringList m_names;
    bool m_hide;
};
}

struct OperationQueue::Work {
    quint64 id = 0;
    Kind kind = Copy;
    QList<QUrl> sources;
    QUrl dest;
    // "Move 3 Items to Backup": what Undo names.
    QString title;
    QList<std::function<KJob *()>> steps;
    int step = 0;
    QPointer<KJob> job;
    // A copy or move on this computer: room and "into itself" are checked first.
    bool preflight = false;
    bool endangered = false;
    // Recorded for Undo when it finishes.
    bool record = true;
    // 0 Undo, 1 Redo when this operation is one; -1 otherwise.
    int side = -1;
    // The window's own dialogs for this one (KIO's paste asks for a name).
    bool widgetDelegate = false;
    QHash<QString, QUrl> pairs;
    QSet<QString> top;
    QList<QUrl> created;
    QUrl lastDest;
    std::function<void(bool)> done;
    // An undo or redo: tells the history queue it is over.
    std::function<void()> histDone;
    QMimeData *data = nullptr;
    bool ended = false;
    // What the steps of an extraction through KIO share.
    std::shared_ptr<ArchivePlan> plan;
    // What a finished job made, to be selected.
    QList<QUrl> results;
};

OperationQueue::OperationQueue(QObject *parent)
    : QAbstractListModel(parent)
    , m_engine(telamon_ops_new())
{
    m_tick.setInterval(250);
    connect(&m_tick, &QTimer::timeout, this, &OperationQueue::tick);
}

OperationQueue::~OperationQueue()
{
    for (auto it = m_work.begin(); it != m_work.end(); ++it) {
        if (it->job) {
            it->job->kill(KJob::Quietly);
        }
        delete it->data;
    }
    telamon_ops_free(m_engine);
}

// ---- The model ----

int OperationQueue::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_ids.size());
}

QHash<int, QByteArray> OperationQueue::roleNames() const
{
    return {
        {IdRole, "opId"},
        {LabelRole, "label"},
        {StateRole, "opState"},
        {ProgressRole, "progress"},
        {DetailRole, "detail"},
        {ErrorRole, "error"},
        {CanPauseRole, "canPause"},
        {CanResumeRole, "canResume"},
        {CanCancelRole, "canCancel"},
        {CanRunNowRole, "canRunNow"},
        {FinishedRole, "finished"},
        {UndoableRole, "undoable"},
    };
}

QVariant OperationQueue::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_ids.size()) {
        return {};
    }
    const quint64 id = m_ids.at(index.row());
    TelamonOpInfo info;
    if (!telamon_ops_info(m_engine, id, &info)) {
        return {};
    }
    const bool finished = info.state >= 4;
    switch (role) {
    case IdRole:
        return id;
    case LabelRole:
        return OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_ops_text(m_engine, id, 0, o, c); });
    case StateRole:
        return stateName(info.state);
    case ProgressRole:
        if (info.state == 4) {
            return 1.0;
        }
        if (info.bytes_total > 0) {
            return double(qMin(info.bytes_done, info.bytes_total)) / double(info.bytes_total);
        }
        if (info.items_total > 1) {
            return double(qMin(info.items_done, info.items_total)) / double(info.items_total);
        }
        return -1.0;
    case DetailRole:
        switch (info.state) {
        case 0:
            return tr("Waiting");
        case 3:
            return tr("Waiting for your answer");
        case 4:
            return tr("Done");
        case 5:
            return tr("Didn't finish");
        case 6:
            return tr("Cancelled");
        default: {
            const QString line = OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_ops_text(m_engine, id, 2, o, c); });
            return info.state == 2 ? (line.isEmpty() ? tr("Paused") : tr("Paused, %1").arg(line)) : line;
        }
        }
    case ErrorRole:
        return OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_ops_text(m_engine, id, 1, o, c); });
    case CanPauseRole:
        return info.state == 1;
    case CanResumeRole:
        return info.state == 2;
    case CanCancelRole:
        return !finished;
    case CanRunNowRole:
        return info.state == 0 && isTransferKind(int(info.kind));
    case FinishedRole:
        return finished;
    case UndoableRole:
        return id == m_undoableId && canUndo() && info.state == 4;
    default:
        return {};
    }
}

int OperationQueue::rowOf(quint64 id) const
{
    return int(m_ids.indexOf(id));
}

// Reads the list of operations from the engine; a changed list resets the model.
void OperationQueue::refresh()
{
    // quint64 and uint64_t are the same width, if not the same type.
    static_assert(sizeof(quint64) == sizeof(uint64_t));
    QList<quint64> ids(32);
    size_t n = telamon_ops_ids(m_engine, reinterpret_cast<uint64_t *>(ids.data()), size_t(ids.size()));
    if (n > size_t(ids.size())) {
        ids.resize(qsizetype(n));
        n = telamon_ops_ids(m_engine, reinterpret_cast<uint64_t *>(ids.data()), size_t(ids.size()));
    }
    ids.resize(qsizetype(n));
    if (ids != m_ids) {
        beginResetModel();
        m_ids = ids;
        endResetModel();
    } else if (!m_ids.isEmpty()) {
        Q_EMIT dataChanged(index(0), index(int(m_ids.size()) - 1));
    }
    Q_EMIT summaryChanged();
    if (active()) {
        if (!m_tick.isActive()) {
            m_tick.start();
        }
    } else {
        m_tick.stop();
    }
}

bool OperationQueue::active() const
{
    for (quint64 id : m_ids) {
        TelamonOpInfo info;
        if (telamon_ops_info(m_engine, id, &info) && info.state < 4) {
            return true;
        }
    }
    return false;
}

double OperationQueue::progress() const
{
    quint64 done = 0, total = 0;
    double fractions = 0;
    int n = 0;
    for (quint64 id : m_ids) {
        TelamonOpInfo info;
        if (!telamon_ops_info(m_engine, id, &info) || info.state == 0 || info.state >= 4) {
            continue;
        }
        if (info.bytes_total > 0) {
            done += qMin(info.bytes_done, info.bytes_total);
            total += info.bytes_total;
        } else if (info.items_total > 1) {
            fractions += double(qMin(info.items_done, info.items_total)) / double(info.items_total);
            ++n;
        }
    }
    if (total > 0) {
        return double(done) / double(total);
    }
    return n > 0 ? fractions / n : -1.0;
}

bool OperationQueue::allPaused() const
{
    bool any = false;
    for (quint64 id : m_ids) {
        TelamonOpInfo info;
        if (!telamon_ops_info(m_engine, id, &info)) {
            continue;
        }
        if (info.state == 1 || info.state == 3) {
            return false;
        }
        any = any || info.state == 2;
    }
    return any;
}

bool OperationQueue::hasFailed() const
{
    for (qsizetype i = m_ids.size() - 1; i >= 0; --i) {
        TelamonOpInfo info;
        if (telamon_ops_info(m_engine, m_ids.at(i), &info) && info.state >= 4) {
            return info.state == 5;
        }
    }
    return false;
}

QString OperationQueue::summary() const
{
    int running = 0, waiting = 0;
    QString first;
    for (quint64 id : m_ids) {
        TelamonOpInfo info;
        if (!telamon_ops_info(m_engine, id, &info)) {
            continue;
        }
        if (info.state == 0) {
            ++waiting;
        } else if (info.state < 4) {
            if (first.isEmpty()) {
                first = OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_ops_text(m_engine, id, 0, o, c); });
            }
            ++running;
        }
    }
    if (running + waiting == 0) {
        return hasFailed() ? tr("An operation didn't finish") : tr("Operations");
    }
    if (first.isEmpty()) {
        return waiting == 1 ? tr("1 operation waiting") : tr("%1 operations waiting").arg(waiting);
    }
    if (waiting == 0) {
        return first;
    }
    return waiting == 1 ? tr("%1 (1 more waiting)").arg(first) : tr("%1 (%2 more waiting)").arg(first).arg(waiting);
}

void OperationQueue::tick()
{
    for (auto it = m_work.begin(); it != m_work.end(); ++it) {
        if (!it->job) {
            continue;
        }
        KJob *j = it->job;
        telamon_ops_progress(m_engine, it.key(), j->processedAmount(KJob::Bytes), j->totalAmount(KJob::Bytes), j->processedAmount(KJob::Files),
                           j->totalAmount(KJob::Files));
    }
    refresh();
}

// ---- Words ----

QString OperationQueue::key(const QUrl &url)
{
    return urlKey(url);
}

QString OperationQueue::folderName(const QUrl &folder)
{
    if (folder.scheme() == QLatin1String("trash")) {
        return tr("Trash");
    }
    if (folder.isLocalFile() && QDir::cleanPath(folder.toLocalFile()) == QDir::cleanPath(QDir::homePath())) {
        return tr("Home");
    }
    const QString name = folder.adjusted(QUrl::StripTrailingSlash).fileName();
    if (!name.isEmpty()) {
        return rustDisplayName(name.toUtf8());
    }
    return folder.isLocalFile() ? QStringLiteral("/") : rustDisplayName(folder.host().toUtf8());
}

QString OperationQueue::nameList(const QList<QUrl> &urls, int max) const
{
    QStringList names;
    for (const QUrl &u : urls.mid(0, max)) {
        names << rustDisplayName(u.adjusted(QUrl::StripTrailingSlash).fileName().toUtf8());
    }
    return names.join(QLatin1Char('\n'));
}

QString OperationQueue::textFor(uint32_t which, Kind kind, const QList<QUrl> &urls, const QString &to, const QString &newName) const
{
    const QByteArray names = nameList(urls, 3).toUtf8(), dst = to.toUtf8(), nn = newName.toUtf8();
    return OpsBridge::textOf([&](uint8_t *o, size_t c) {
        return telamon_op_text(which, uint32_t(kind), OpsBridge::p(names), OpsBridge::n(names), size_t(urls.size()), OpsBridge::p(dst), OpsBridge::n(dst),
                             OpsBridge::p(nn), OpsBridge::n(nn), o, c);
    });
}

OperationQueue::Work &OperationQueue::work(quint64 id)
{
    return m_work[id];
}

// ---- Putting work in the queue ----

quint64 OperationQueue::enqueue(Work w, const QString &runningLabel)
{
    const QByteArray label = runningLabel.toUtf8();
    const quint64 id = telamon_ops_add(m_engine, uint32_t(w.kind), OpsBridge::p(label), OpsBridge::n(label));
    if (id == 0) {
        return 0;
    }
    w.id = id;
    m_work.insert(id, std::move(w));
    pump();
    refresh();
    return id;
}

// Carries out what the core says: start, suspend, resume or kill a job.
void OperationQueue::pump()
{
    uint32_t kind = 0;
    uint64_t id = 0;
    while (telamon_ops_next_action(m_engine, &kind, &id)) {
        if (!m_work.contains(id)) {
            continue;
        }
        switch (kind) {
        case 0:
            startOp(id);
            break;
        case 1:
            if (work(id).job) {
                work(id).job->suspend();
            }
            break;
        case 2:
            if (work(id).job) {
                work(id).job->resume();
            }
            break;
        default: {
            Work &w = work(id);
            if (w.job) {
                w.job->kill(KJob::Quietly);
            }
            // A copy killed half way leaves its partial file; KIO removes
            // the ".part" it writes first, and nothing else is left.
            endOp(id);
            break;
        }
        }
    }
}

void OperationQueue::startOp(quint64 id)
{
    Work &w = work(id);
    if (!w.preflight) {
        runStep(id);
        return;
    }
    // Room and "into itself" for a local copy or move, read from the disk on
    // a worker; the job starts when they pass.
    QByteArray sources;
    for (const QUrl &u : w.sources) {
        sources += u.toLocalFile().toUtf8();
        sources.append('\0');
    }
    const QByteArray dest = w.dest.toLocalFile().toUtf8();
    const uint32_t transfer = w.kind == Copy ? 0 : 1;
    const QString title = w.kind == Copy ? tr("Can't Copy") : tr("Can't Move");
    QPointer<OperationQueue> self(this);
    QThreadPool::globalInstance()->start([self, id, sources, dest, transfer, title] {
        size_t len = 0;
        QByteArray out(512, 0);
        auto call = [&] {
            return telamon_preflight(transfer, OpsBridge::p(sources), OpsBridge::n(sources), OpsBridge::p(dest), OpsBridge::n(dest), -1,
                                   reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()), &len);
        };
        int rc = call();
        if (len > size_t(out.size())) {
            out.resize(qsizetype(len));
            rc = call();
        }
        const QString why = QString::fromUtf8(out.constData(), qsizetype(qMin(len, size_t(out.size()))));
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(self.data(), [self, id, rc, why, title] {
            if (!self || !self->m_work.contains(id)) {
                return;
            }
            if (rc == 0) {
                self->runStep(id);
            } else {
                // The dialog says it; the list keeps the row, the toast stays quiet.
                Q_EMIT self->refused(title, why);
                self->failOp(id, why, false);
            }
        });
    });
}

void OperationQueue::attach(Work &w, KJob *job)
{
    const quint64 id = w.id;
    w.job = job;
    if (!w.widgetDelegate) {
        auto *delegate = new KJobUiDelegate;
        new OperationAsker(this, id, delegate);
        job->setUiDelegate(delegate);
    }
    connect(job, &KJob::result, this, [this, id](KJob *j) { stepDone(id, j); });
    if (auto *archiveJob = qobject_cast<ArchiveJob *>(job)) {
        connect(archiveJob, &ArchiveJob::needsUser, this, [this] { Q_EMIT message(tr("Telamon Archive needs an answer from you. Look for its window.")); });
    }
    if (auto *copy = qobject_cast<KIO::CopyJob *>(job)) {
        connect(copy, &KIO::CopyJob::copyingDone, this, [this, id](KIO::Job *, const QUrl &from, const QUrl &to, const QDateTime &, bool, bool) {
            if (m_work.contains(id) && work(id).top.contains(urlKey(from))) {
                work(id).pairs.insert(urlKey(from), to);
            }
        });
        connect(copy, &KIO::CopyJob::copyingLinkDone, this, [this, id](KIO::Job *, const QUrl &from, const QString &, const QUrl &to) {
            if (m_work.contains(id) && work(id).top.contains(urlKey(from))) {
                work(id).pairs.insert(urlKey(from), to);
            }
        });
        connect(copy, &KIO::CopyJob::copying, this, [this, id](KIO::Job *, const QUrl &, const QUrl &to) {
            if (m_work.contains(id)) {
                work(id).lastDest = to;
            }
        });
    }
    telamon_ops_started(m_engine, id);
    // Paused while it was being checked (before there was a job to suspend).
    TelamonOpInfo info;
    if (telamon_ops_info(m_engine, id, &info) && info.state == 2) {
        job->suspend();
    }
}

void OperationQueue::runStep(quint64 id)
{
    if (!m_work.contains(id)) {
        return;
    }
    Work &w = work(id);
    if (w.step >= w.steps.size()) {
        finishOp(id);
        return;
    }
    KJob *job = w.steps.at(w.step)();
    if (!job) {
        failOp(id, tr("This can't be done here."));
        return;
    }
    attach(w, job);
}

void OperationQueue::stepDone(quint64 id, KJob *job)
{
    if (!m_work.contains(id)) {
        return;
    }
    Work &w = work(id);
    w.job = nullptr;
    if (job->error()) {
        if (job->error() == KIO::ERR_USER_CANCELED || job->error() == KJob::KilledJobError) {
            cancel(id);
            return;
        }
        // An archive Files won't take apart, or a disk without room: a dialog
        // says it in full (the list keeps the row, the toast stays quiet).
        const bool ours = qobject_cast<ArchiveGuardJob *>(job) || qobject_cast<ArchivePrepareJob *>(job);
        if (ours && (job->error() == ArchiveGuardJob::Refused || job->error() == ArchiveGuardJob::NeedsPassword || job->error() == ArchivePrepareJob::NoRoom)) {
            const QString text = job->errorString();
            Q_EMIT refused(job->error() == ArchiveGuardJob::Refused ? tr("Can't Extract")
                                                                    : (job->error() == ArchiveGuardJob::NeedsPassword ? tr("Needs a Password") : tr("Not Enough Space")),
                           text);
            failOp(id, text, false);
            return;
        }
        failOp(id, rustDisplayName(job->errorString().toUtf8()));
        return;
    }
    if (auto *archiveJob = qobject_cast<ArchiveJob *>(job)) {
        w.results = archiveJob->results();
    }
    ++w.step;
    runStep(id);
}

// The last step is done: the core moves on, and what can be undone is recorded.
void OperationQueue::finishOp(quint64 id)
{
    telamon_ops_event(m_engine, id, 4);
    pump();
    const Work w = work(id);
    endOp(id, true);
    if (w.side >= 0) {
        completeHistory(w);
    } else if (w.endangered) {
        // The result holds files that were there before: trashing "what was
        // created" would take them along. Nothing before it can be undone
        // through it either.
        telamon_hist_barrier(m_engine);
        refreshHistory();
        Q_EMIT message(tr("%1 replaced or merged files, so it can't be undone.").arg(w.title));
    } else if (w.record) {
        recordHistory(w);
    }
    if (w.kind == EmptyTrash) {
        Q_EMIT message(tr("Trash emptied."));
    }
    refresh();
    Q_EMIT jobFinished();
    const QList<QUrl> made = w.plan ? w.plan->results : w.results;
    if (!made.isEmpty()) {
        Q_EMIT resultsReady(made);
    }
}

void OperationQueue::failOp(quint64 id, const QString &why, bool say)
{
    if (!m_work.contains(id)) {
        return;
    }
    const Work w = work(id);
    const QByteArray reason = why.toUtf8();
    telamon_ops_failed(m_engine, id, OpsBridge::p(reason), OpsBridge::n(reason));
    pump();
    if (w.job) {
        w.job->kill(KJob::Quietly);
    }
    if (w.side >= 0) {
        // The undo or redo did not go through: its entry is stale.
        telamon_hist_drop(m_engine, uint32_t(w.side));
        m_undoableId = 0;
        refreshHistory();
        Q_EMIT message(tr("Couldn't %1 \"%2\": %3").arg(w.side == 0 ? tr("undo") : tr("redo"), w.title, why));
    } else if (say) {
        Q_EMIT message(tr("%1 didn't finish: %2").arg(w.title, why));
    }
    endOp(id, false);
    // A batch of renames that stopped half way: the ones done are one step to undo.
    if (w.kind == Rename && w.record && w.side < 0 && w.sources.size() > 1 && !w.pairs.isEmpty()) {
        recordHistory(w);
    }
    refresh();
    Q_EMIT jobFinished();
}

// Drops what the queue kept for an operation that has ended (the core keeps
// its row). `ok`: it did what was asked.
void OperationQueue::endOp(quint64 id, bool ok)
{
    removeQuestions(id);
    auto it = m_work.find(id);
    if (it == m_work.end()) {
        return;
    }
    delete it->data;
    auto doneFn = std::move(it->done);
    // A finished undo or redo reports itself when its entry is moved
    // (completeHistory); one that failed or was cancelled is over now.
    auto histFn = ok ? std::function<void()>() : std::move(it->histDone);
    m_work.erase(it);
    if (doneFn) {
        doneFn(ok);
    }
    if (histFn) {
        histFn();
    }
    refresh();
}

// ---- The user's side ----

void OperationQueue::pause(quint64 id)
{
    telamon_ops_event(m_engine, id, 0);
    pump();
    refresh();
}

void OperationQueue::resume(quint64 id)
{
    telamon_ops_event(m_engine, id, 1);
    pump();
    refresh();
}

void OperationQueue::runNow(quint64 id)
{
    telamon_ops_event(m_engine, id, 2);
    pump();
    refresh();
}

void OperationQueue::cancel(quint64 id)
{
    const bool known = m_work.contains(id);
    telamon_ops_event(m_engine, id, 3);
    pump();
    if (known && m_work.contains(id)) {
        // A batch of renames stopped half way: the ones done are one step to undo.
        const Work w = work(id);
        const bool partial = w.kind == Rename && w.record && w.side < 0 && w.sources.size() > 1 && !w.pairs.isEmpty();
        // Waiting operations have no job to kill.
        endOp(id);
        if (partial) {
            recordHistory(w);
        }
    }
    if (known) {
        Q_EMIT jobFinished();
    }
    refresh();
}

void OperationQueue::dismiss(quint64 id)
{
    telamon_ops_dismiss(m_engine, id);
    refresh();
}

void OperationQueue::removeQuestions(quint64 id)
{
    const int before = int(m_questions.size());
    m_questions.removeIf([id](const Question &q) { return q.opId == id; });
    if (before != m_questions.size()) {
        Q_EMIT questionChanged();
    }
}

void OperationQueue::answer(const QVariantMap &reply)
{
    if (m_questions.isEmpty()) {
        return;
    }
    Question q = m_questions.takeFirst();
    Q_EMIT questionChanged();
    if (q.reply) {
        q.reply(reply);
    }
    refresh();
}

QString OperationQueue::checkName(const QString &name, const QString &existingName) const
{
    if (name == existingName) {
        return tr("Choose a name that is not taken.");
    }
    const RustCheck check = rustValidateName(name);
    return check.ok ? QString() : check.text;
}

// ---- Questions from the jobs ----

namespace
{
QVariantMap sideOf(const QUrl &url, bool isDir, KIO::filesize_t size, const QDateTime &when)
{
    QVariantMap m;
    const QString name = url.adjusted(QUrl::StripTrailingSlash).fileName();
    m.insert(QStringLiteral("name"), rustDisplayName(name.toUtf8()));
    m.insert(QStringLiteral("isDir"), isDir);
    m.insert(QStringLiteral("sizeText"), isDir || size == KIO::filesize_t(-1) ? QString() : KIO::convertSize(size));
    m.insert(QStringLiteral("dateText"), when.isValid() ? QLocale().toString(when, QLocale::ShortFormat) : QString());
    m.insert(QStringLiteral("seconds"), when.isValid() ? when.toSecsSinceEpoch() : qint64(0));
    m.insert(QStringLiteral("hasDate"), when.isValid());
    QString icon = isDir ? QStringLiteral("folder") : QMimeDatabase().mimeTypeForFile(name, QMimeDatabase::MatchExtension).iconName();
    m.insert(QStringLiteral("icon"), icon.isEmpty() ? QStringLiteral("application-octet-stream") : icon);
    QString thumb;
    if (!isDir && url.isLocalFile()) {
        thumb = QStringLiteral("image://thumb/") + QString::fromLatin1(url.toEncoded().toBase64(QByteArray::Base64UrlEncoding | QByteArray::OmitTrailingEquals))
            + QLatin1Char('/') + QString::number(when.isValid() ? when.toSecsSinceEpoch() : 0);
    }
    m.insert(QStringLiteral("thumb"), thumb);
    // Where it is, in words: the folder's name.
    const QUrl parent = url.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    m.insert(QStringLiteral("where"), parent.scheme() == QLatin1String("trash") ? QObject::tr("Trash") : rustDisplayName(parent.adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()));
    return m;
}

enum Answer { Replace = 0, Skip = 1, KeepBoth = 2, Merge = 3 };
}

void OperationQueue::askConflict(OperationAsker *asker, quint64 opId, KJob *job, const QUrl &src, const QUrl &dest, KIO::RenameDialog_Options options,
                                 KIO::filesize_t sizeSrc, KIO::filesize_t sizeDest, const QDateTime &mtimeSrc, const QDateTime &mtimeDest,
                                 const QDateTime &ctimeSrc, const QDateTime &ctimeDest)
{
    const bool srcDir = options.testFlag(KIO::RenameDialog_SourceIsDirectory);
    const bool dstDir = options.testFlag(KIO::RenameDialog_DestIsDirectory);
    const bool same = options.testFlag(KIO::RenameDialog_OverwriteItself);
    const bool multiple = options.testFlag(KIO::RenameDialog_MultipleItems);
    QPointer<OperationAsker> a(asker);
    QPointer<KJob> j(job);

    // The new name for Keep Both, in the destination's folder.
    const QUrl folder = dest.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    auto suggestion = [dest, folder] {
        const QByteArray dir = folder.isLocalFile() ? folder.toLocalFile().toUtf8() : QByteArray();
        const QByteArray name = dest.adjusted(QUrl::StripTrailingSlash).fileName().toUtf8();
        return OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_keep_both(OpsBridge::p(dir), OpsBridge::n(dir), OpsBridge::p(name), OpsBridge::n(name), o, c); });
    };
    auto inFolder = [folder](const QString &name) {
        QUrl u = folder;
        u.setPath(QDir::cleanPath(folder.path() + QLatin1Char('/') + name));
        return u;
    };
    // Gives the job its answer. Replace and merge touch what was there before.
    auto reply = [this, opId, a, j, dest, inFolder, suggestion](uint32_t ans, const QString &name) {
        if (!a || !j) {
            return;
        }
        if (telamon_conflict_endangers(ans) && m_work.contains(opId)) {
            work(opId).endangered = true;
        }
        switch (ans) {
        case Replace:
        case Merge:
            a->replyRename(KIO::Result_Overwrite, dest, j);
            break;
        case KeepBoth: {
            // The dialog checks the name; this is the second look.
            const QString use = rustValidateName(name).ok ? name : suggestion();
            a->replyRename(KIO::Result_Rename, inFolder(use), j);
            break;
        }
        default:
            a->replyRename(KIO::Result_Skip, dest, j);
            break;
        }
    };

    // An undo or redo never overwrites anything: something that turned up
    // since it was planned is left alone.
    if (m_work.contains(opId) && work(opId).side >= 0) {
        asker->replyRename(KIO::Result_Skip, dest, job);
        return;
    }
    const uint32_t code = telamon_ops_conflict(m_engine, opId, srcDir && dstDir ? 2 : 1);
    if (code == 0) {
        asker->replyRename(KIO::Result_Cancel, dest, job);
        return;
    }
    if (code >= 2) {
        // "Do this for all conflicts" was answered before. An answer that
        // doesn't fit this one (replace for a folder) skips it.
        uint32_t ans = code - 2;
        if (!telamon_conflict_allowed(srcDir, dstDir, same, ans)) {
            ans = Skip;
        }
        reply(ans, ans == KeepBoth ? suggestion() : QString());
        return;
    }

    const uint32_t choices = telamon_conflict_choices(srcDir, dstDir, same);
    QVariantMap q;
    q.insert(QStringLiteral("type"), QStringLiteral("conflict"));
    q.insert(QStringLiteral("opId"), opId);
    q.insert(QStringLiteral("folders"), srcDir && dstDir);
    q.insert(QStringLiteral("mismatch"), srcDir != dstDir);
    q.insert(QStringLiteral("same"), same);
    q.insert(QStringLiteral("multiple"), multiple);
    q.insert(QStringLiteral("replace"), (choices & 1) != 0);
    q.insert(QStringLiteral("merge"), (choices & 2) != 0);
    q.insert(QStringLiteral("keepBoth"), (choices & 4) != 0);
    q.insert(QStringLiteral("skip"), (choices & 8) != 0);
    static const char *const names[] = {"replace", "skip", "keepBoth", "merge"};
    q.insert(QStringLiteral("defaultAnswer"), QLatin1String(names[(choices >> 8) & 3]));
    q.insert(QStringLiteral("suggested"), suggestion());
    q.insert(QStringLiteral("existingName"), dest.adjusted(QUrl::StripTrailingSlash).fileName());
    const Kind kind = m_work.contains(opId) ? work(opId).kind : Copy;
    q.insert(QStringLiteral("moving"), kind == Move || kind == Rename || kind == Restore);
    q.insert(QStringLiteral("verb"), kind == Move ? tr("Moving") : (kind == Rename ? tr("Renaming") : (kind == Restore ? tr("Restoring") : tr("Copying"))));
    const QDateTime whenSrc = mtimeSrc.isValid() ? mtimeSrc : ctimeSrc;
    const QDateTime whenDst = mtimeDest.isValid() ? mtimeDest : ctimeDest;
    QVariantMap from = sideOf(src, srcDir, sizeSrc, whenSrc), to = sideOf(dest, dstDir, sizeDest, whenDst);
    const uint32_t newer = telamon_conflict_newer(whenSrc.isValid(), whenSrc.toSecsSinceEpoch(), whenDst.isValid(), whenDst.toSecsSinceEpoch());
    from.insert(QStringLiteral("newer"), newer == 1);
    to.insert(QStringLiteral("newer"), newer == 2);
    q.insert(QStringLiteral("source"), from);
    q.insert(QStringLiteral("dest"), to);

    Question question;
    question.opId = opId;
    question.data = q;
    question.reply = [this, opId, reply, srcDir, dstDir, same](const QVariantMap &r) {
        const QString what = r.value(QStringLiteral("answer")).toString();
        if (what == QLatin1String("cancel")) {
            cancel(opId);
            return;
        }
        uint32_t ans = what == QLatin1String("replace") ? Replace : what == QLatin1String("keepBoth") ? KeepBoth : what == QLatin1String("merge") ? Merge : Skip;
        if (!telamon_conflict_allowed(srcDir, dstDir, same, ans)) {
            ans = Skip;
        }
        // The core wakes the operation and remembers "for all".
        telamon_ops_answer(m_engine, opId, ans, r.value(QStringLiteral("all")).toBool());
        reply(ans, r.value(QStringLiteral("name")).toString());
    };
    m_questions.append(question);
    if (m_questions.size() == 1) {
        Q_EMIT questionChanged();
    }
    pump();
    refresh();
}

void OperationQueue::askProblem(OperationAsker *asker, quint64 opId, KJob *job, KIO::SkipDialog_Options options, const QString &text)
{
    QPointer<OperationAsker> a(asker);
    QPointer<KJob> j(job);
    QVariantMap q;
    q.insert(QStringLiteral("type"), QStringLiteral("problem"));
    q.insert(QStringLiteral("opId"), opId);
    q.insert(QStringLiteral("text"), rustDisplayName(text.toUtf8()));
    q.insert(QStringLiteral("canRetry"), !options.testFlag(KIO::SkipDialog_Hide_Retry));
    q.insert(QStringLiteral("multiple"), options.testFlag(KIO::SkipDialog_MultipleItems));
    Question question;
    question.opId = opId;
    question.data = q;
    question.reply = [this, opId, a, j](const QVariantMap &r) {
        const QString what = r.value(QStringLiteral("answer")).toString();
        if (!a || !j) {
            return;
        }
        if (what == QLatin1String("retry")) {
            a->replySkip(KIO::Result_Retry, j);
        } else if (what == QLatin1String("skip")) {
            a->replySkip(KIO::Result_Skip, j);
        } else if (what == QLatin1String("skipAll")) {
            a->replySkip(KIO::Result_AutoSkip, j);
        } else {
            a->replySkip(KIO::Result_Cancel, j);
            cancel(opId);
        }
    };
    m_questions.append(question);
    if (m_questions.size() == 1) {
        Q_EMIT questionChanged();
    }
}

void OperationQueue::askDelete(OperationAsker *asker, quint64 opId, const QList<QUrl> &urls, KIO::AskUserActionInterface::DeletionType type)
{
    QPointer<OperationAsker> a(asker);
    QVariantMap q;
    q.insert(QStringLiteral("type"), QStringLiteral("delete"));
    q.insert(QStringLiteral("opId"), opId);
    const QString what = urls.size() == 1 ? tr("\"%1\"").arg(rustDisplayName(urls.first().adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()))
                                          : tr("These %n items", "", int(urls.size()));
    q.insert(QStringLiteral("text"),
             type == KIO::AskUserActionInterface::Trash || type == KIO::AskUserActionInterface::EmptyTrash
                 ? tr("Delete %1 for good? This can't be undone.").arg(what)
                 : tr("%1 can't be moved to the Trash. Delete for good instead? This can't be undone.").arg(what));
    Question question;
    question.opId = opId;
    question.data = q;
    question.reply = [a, urls, type](const QVariantMap &r) {
        if (a) {
            a->replyDelete(r.value(QStringLiteral("answer")).toString() == QLatin1String("yes"), urls, type);
        }
    };
    m_questions.append(question);
    if (m_questions.size() == 1) {
        Q_EMIT questionChanged();
    }
}

// ---- What the app asks for ----

void OperationQueue::transfer(Kind kind, const QList<QUrl> &sourcesIn, const QUrl &destination, std::function<void(bool)> done)
{
    QList<QUrl> sources;
    QSet<QString> seen;
    for (const QUrl &u : sourcesIn) {
        if (u.isValid() && !seen.contains(key(u))) {
            seen.insert(key(u));
            sources << u;
        }
    }
    if (sources.isEmpty() || !destination.isValid()) {
        return;
    }
    // A folder into itself is refused before anything is queued.
    if (kind != Link) {
        for (const QUrl &u : sources) {
            const QByteArray s = key(u).toUtf8(), d = key(destination).toUtf8();
            if (telamon_is_inside(OpsBridge::p(s), OpsBridge::n(s), OpsBridge::p(d), OpsBridge::n(d))) {
                const QByteArray name = rustDisplayName(u.adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()).toUtf8();
                const QByteArray dest = folderName(destination).toUtf8();
                const QString text = OpsBridge::textOf([&](uint8_t *o, size_t c) {
                    return telamon_into_itself_text(kind == Copy ? 0 : 1, OpsBridge::p(name), OpsBridge::n(name), OpsBridge::p(dest), OpsBridge::n(dest),
                                                    s == d, o, c);
                });
                Q_EMIT refused(kind == Copy ? tr("Can't Copy") : tr("Can't Move"), text);
                if (done) {
                    done(false);
                }
                return;
            }
        }
    }

    // Archives are read-only here: nothing goes into one, and what comes out
    // is copied (the archive keeps its files), after Files has looked at
    // what the archive lists.
    bool fromArchive = false;
    for (const QUrl &u : sources) {
        fromArchive = fromArchive || rustArchiveScheme(u.scheme());
    }
    if (fromArchive && !std::all_of(sources.cbegin(), sources.cend(), [](const QUrl &u) { return rustArchiveScheme(u.scheme()); })) {
        Q_EMIT refused(tr("Can't Copy"), tr("Files in an archive and files outside it can't be taken in one go. Do them one after the other."));
        if (done) {
            done(false);
        }
        return;
    }
    if (rustArchiveScheme(destination.scheme())) {
        Q_EMIT refused(tr("Can't Copy"), tr("An archive is open for reading only, so nothing can be put into it. Extract it first, then change the files."));
        if (done) {
            done(false);
        }
        return;
    }
    if (fromArchive && kind == Link) {
        Q_EMIT refused(tr("Can't Link"), tr("A link can't point to a file in an archive. Extract it first."));
        if (done) {
            done(false);
        }
        return;
    }
    if (fromArchive) {
        kind = Copy;
    }

    Work w;
    w.kind = kind;
    w.sources = sources;
    w.dest = destination;
    w.done = std::move(done);
    for (const QUrl &u : sources) {
        w.top.insert(key(u));
    }
    bool local = destination.isLocalFile();
    for (const QUrl &u : sources) {
        local = local && u.isLocalFile();
    }
    w.preflight = local && (kind == Copy || kind == Move);
    const QString to = folderName(destination);
    w.title = textFor(0, kind, sources, to, QString());
    if (fromArchive) {
        // Look first, check the room, then copy; there is nothing to undo
        // that redo could safely repeat (it would skip the look).
        w.record = false;
        auto plan = std::make_shared<ArchivePlan>();
        const QString what = sources.size() == 1 ? tr("\"%1\"").arg(rustDisplayName(sources.first().adjusted(QUrl::StripTrailingSlash).fileName().toUtf8()))
                                                 : tr("%n items", "", int(sources.size()));
        w.steps << [this, sources, plan]() -> KJob * { return new ArchiveGuardJob(sources, false, plan, archiveInstalled()); };
        w.steps << [destination, plan, what]() -> KJob * { return new ArchivePrepareJob(destination, QString(), what, plan); };
    }
    w.steps << [sources, destination, kind]() -> KJob * {
        switch (kind) {
        case Copy:
            return KIO::copy(sources, destination, KIO::HideProgressInfo);
        case Move:
            return KIO::move(sources, destination, KIO::HideProgressInfo);
        default:
            return KIO::link(sources, destination, KIO::HideProgressInfo);
        }
    };
    enqueue(std::move(w), textFor(1, kind, sources, to, QString()));
}

void OperationQueue::trash(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    Work w;
    w.kind = Trash;
    w.sources = urls;
    for (const QUrl &u : urls) {
        w.top.insert(key(u));
    }
    w.title = textFor(0, Trash, urls, QString(), QString());
    w.steps << [urls]() -> KJob * { return KIO::trash(urls, KIO::HideProgressInfo); };
    enqueue(std::move(w), textFor(1, Trash, urls, QString(), QString()));
}

void OperationQueue::deleteForGood(const QList<QUrl> &urls)
{
    if (urls.isEmpty()) {
        return;
    }
    Work w;
    w.kind = Delete;
    w.sources = urls;
    w.record = false;
    w.title = textFor(0, Delete, urls, QString(), QString());
    w.steps << [urls]() -> KJob * { return KIO::del(urls, KIO::HideProgressInfo); };
    enqueue(std::move(w), textFor(1, Delete, urls, QString(), QString()));
}

void OperationQueue::rename(const QUrl &url, const QString &newName, std::function<void(bool)> done)
{
    QUrl target = url.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    target.setPath(QDir::cleanPath(target.path() + QLatin1Char('/') + newName));
    Work w;
    w.kind = Rename;
    w.sources = {url};
    w.dest = target;
    w.done = std::move(done);
    w.top.insert(key(url));
    w.title = textFor(0, Rename, {url}, QString(), rustDisplayName(newName.toUtf8()));
    // A move job: KIO::rename has no conflict handling and reports no result.
    w.steps << [url, target]() -> KJob * { return KIO::moveAs(url, target, KIO::HideProgressInfo); };
    enqueue(std::move(w), textFor(1, Rename, {url}, QString(), rustDisplayName(newName.toUtf8())));
}

void OperationQueue::renameMany(const QList<QPair<QUrl, QString>> &renames, std::function<void(bool)> done)
{
    if (renames.isEmpty()) {
        if (done) {
            done(false);
        }
        return;
    }
    if (renames.size() == 1) {
        rename(renames.first().first, renames.first().second, std::move(done));
        return;
    }
    Work w;
    w.kind = Rename;
    w.done = std::move(done);
    for (const auto &r : renames) {
        const QUrl url = r.first;
        QUrl target = url.adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
        target.setPath(QDir::cleanPath(target.path() + QLatin1Char('/') + r.second));
        w.sources << url;
        w.top.insert(key(url));
        // One step each: they run in order, and each is the single rename's move job.
        w.steps << [url, target]() -> KJob * { return KIO::moveAs(url, target, KIO::HideProgressInfo); };
    }
    w.dest = w.sources.first().adjusted(QUrl::StripTrailingSlash | QUrl::RemoveFilename);
    w.title = textFor(0, Rename, w.sources, QString(), QString());
    const QString running = textFor(1, Rename, w.sources, QString(), QString());
    enqueue(std::move(w), running);
}

void OperationQueue::makeFolder(const QUrl &folder, const QString &name, std::function<void(bool)> done)
{
    QUrl target = folder.adjusted(QUrl::StripTrailingSlash);
    target.setPath(QDir::cleanPath(target.path() + QLatin1Char('/') + name));
    Work w;
    w.kind = NewFolder;
    w.dest = target;
    w.done = std::move(done);
    w.created = {target};
    w.title = tr("Create Folder %1").arg(rustDisplayName(name.toUtf8()));
    w.steps << [target]() -> KJob * { return KIO::mkdir(target); };
    enqueue(std::move(w), tr("Creating folder %1").arg(rustDisplayName(name.toUtf8())));
}

void OperationQueue::makeFile(const QUrl &folder, const QString &name, const QUrl &templateFile, std::function<void(bool)> done)
{
    QUrl target = folder.adjusted(QUrl::StripTrailingSlash);
    target.setPath(QDir::cleanPath(target.path() + QLatin1Char('/') + name));
    Work w;
    w.kind = Copy;
    w.dest = target;
    w.done = std::move(done);
    // Recorded as a copy, so Undo moves the new file to the Trash and Redo
    // brings it back; an empty file has no source, so it stands for itself.
    const QUrl from = templateFile.isValid() ? templateFile : target;
    w.sources = {from};
    w.top.insert(key(from));
    w.pairs.insert(key(from), target);
    w.title = tr("Create File %1").arg(rustDisplayName(name.toUtf8()));
    if (templateFile.isValid()) {
        w.steps << [templateFile, target]() -> KJob * { return KIO::file_copy(templateFile, target, -1, KIO::HideProgressInfo); };
    } else {
        w.steps << [target]() -> KJob * { return KIO::storedPut(QByteArray(), target, -1, KIO::HideProgressInfo); };
    }
    enqueue(std::move(w), tr("Creating file %1").arg(rustDisplayName(name.toUtf8())));
}

void OperationQueue::setHidden(const QList<QUrl> &urls, bool hide, std::function<void(bool)> done)
{
    QStringList names;
    QString dir;
    for (const QUrl &u : urls) {
        if (!u.isLocalFile()) {
            continue;
        }
        const QFileInfo info(u.adjusted(QUrl::StripTrailingSlash).toLocalFile());
        if (dir.isEmpty()) {
            dir = info.absolutePath();
        }
        // One folder's file: items from elsewhere are not touched.
        if (info.absolutePath() == dir && !names.contains(info.fileName())) {
            names << info.fileName();
        }
    }
    if (names.isEmpty()) {
        return;
    }
    Work w;
    w.kind = Rename;
    w.record = false;
    w.done = std::move(done);
    const QString what = names.size() == 1 ? rustDisplayName(names.first().toUtf8()) : tr("%1 items").arg(names.size());
    w.title = hide ? tr("Hide %1").arg(what) : tr("Show %1").arg(what);
    w.steps << [dir, names, hide]() -> KJob * { return new HiddenFileJob(dir, names, hide); };
    enqueue(std::move(w), hide ? tr("Hiding %1").arg(what) : tr("Showing %1").arg(what));
}

void OperationQueue::pasteData(const QMimeData *data, const QUrl &destination)
{
    if (!data || !destination.isValid()) {
        return;
    }
    // The clipboard may change before the job asks its question: keep a copy.
    auto *copy = new QMimeData;
    for (const QString &format : data->formats()) {
        copy->setData(format, data->data(format));
    }
    Work w;
    w.kind = Copy;
    w.dest = destination;
    w.record = false;
    w.widgetDelegate = true;
    w.data = copy;
    w.title = tr("Paste");
    w.steps << [this, copy, destination]() -> KJob * {
        KIO::PasteJob *job = KIO::paste(copy, destination, KIO::HideProgressInfo);
        if (job) {
            job->setUiDelegate(KIO::createDefaultJobUiDelegate(KJobUiDelegate::AutoHandlingEnabled, nullptr));
        }
        return job;
    };
    enqueue(std::move(w), tr("Pasting into %1").arg(folderName(destination)));
}

void OperationQueue::emptyTrash()
{
    Work w;
    w.kind = EmptyTrash;
    w.record = false;
    w.title = tr("Empty Trash");
    w.steps << []() -> KJob * { return KIO::emptyTrash(); };
    enqueue(std::move(w), tr("Emptying the Trash"));
}

void OperationQueue::archive(const QString &method, const QVariantList &args, const QVariantMap &options, const QString &title, const QString &running)
{
    Work w;
    w.kind = External;
    w.record = false;
    w.title = title;
    w.steps << [method, args, options]() -> KJob * { return new ArchiveJob(method, args, options); };
    enqueue(std::move(w), running);
}

void OperationQueue::extractArchive(const QUrl &root, const QUrl &parentFolder, const QString &folderName, const QString &archiveName)
{
    auto plan = std::make_shared<ArchivePlan>();
    Work w;
    w.kind = Copy;
    w.record = false;
    w.sources = {root};
    w.plan = plan;
    const QString shown = rustDisplayName(archiveName.toUtf8());
    w.title = tr("Extract \"%1\"").arg(shown);
    w.steps << [this, root, plan]() -> KJob * { return new ArchiveGuardJob({root}, true, plan, archiveInstalled()); };
    w.steps << [parentFolder, folderName, shown, plan]() -> KJob * { return new ArchivePrepareJob(parentFolder, folderName, tr("\"%1\"").arg(shown), plan); };
    w.steps << [plan]() -> KJob * {
        plan->results = {plan->dest};
        return KIO::mkdir(plan->dest);
    };
    w.steps << [plan]() -> KJob * {
        if (plan->children.isEmpty()) {
            return new NoopJob;
        }
        return KIO::copy(plan->children, plan->dest, KIO::HideProgressInfo);
    };
    enqueue(std::move(w), tr("Extracting \"%1\"").arg(shown));
}

// ---- Undo and redo ----

void OperationQueue::refreshHistory()
{
    auto titles = [&](uint32_t side) {
        const QString all = OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_hist_titles(m_engine, side, o, c); });
        return all.isEmpty() ? QStringList() : all.split(QLatin1Char('\n'));
    };
    m_undoTitles = titles(0);
    m_redoTitles = titles(1);
    Q_EMIT historyChanged();
}

void OperationQueue::undo()
{
    startHistory(0);
}

void OperationQueue::redo()
{
    startHistory(1);
}

void OperationQueue::histEnqueue(HistoryJob job)
{
    m_histJobs.append(std::move(job));
    histPump();
}

void OperationQueue::histPump()
{
    if (m_histRunning || m_histJobs.isEmpty()) {
        return;
    }
    m_histRunning = true;
    HistoryJob job = m_histJobs.takeFirst();
    QPointer<OperationQueue> self(this);
    job([self] {
        if (self) {
            self->m_histRunning = false;
            self->histPump();
        }
    });
}

void OperationQueue::startHistory(int side)
{
    // A press per second for a long time is not an undo list.
    if (m_histJobs.size() >= 20) {
        Q_EMIT message(tr("Wait for the last undo or redo to finish."));
        return;
    }
    QPointer<OperationQueue> self(this);
    histEnqueue([self, side](std::function<void()> next) {
        if (self) {
            self->runHistory(side, std::move(next));
        }
    });
}

// Plans the next undo (or redo) and queues it; `next` is called when it is
// over, or at once when there is nothing to run.
void OperationQueue::runHistory(int side, std::function<void()> next)
{
    const QStringList titles = side == 0 ? m_undoTitles : m_redoTitles;
    if (titles.isEmpty()) {
        Q_EMIT message(side == 0 ? tr("There is nothing to undo.") : tr("There is nothing to redo."));
        next();
        return;
    }
    const QString title = titles.first();
    const QString paths = OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_hist_paths(m_engine, uint32_t(side), false, o, c); });
    QList<QUrl> urls;
    for (const QString &p : paths.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        urls << urlOfKey(p);
    }
    QPointer<OperationQueue> self(this);
    StatBatch::run(urls, this, [self, side, title, next](const SeenMap &seen) {
        if (!self) {
            return;
        }
        QString states;
        for (auto it = seen.begin(); it != seen.end(); ++it) {
            states += stateLine(it.key(), it.value());
        }
        const QByteArray st = states.toUtf8();
        size_t len = 0;
        QByteArray out(1024, 0);
        auto call = [&] {
            return telamon_hist_plan(self->m_engine, uint32_t(side), OpsBridge::p(st), OpsBridge::n(st), reinterpret_cast<uint8_t *>(out.data()), size_t(out.size()),
                                   &len);
        };
        int rc = call();
        if (len > size_t(out.size())) {
            out.resize(qsizetype(len));
            rc = call();
        }
        const QString text = QString::fromUtf8(out.constData(), qsizetype(qMin(len, size_t(out.size()))));
        const QString verb = side == 0 ? tr("undo") : tr("redo");
        if (rc != 0) {
            // Why not, in words; the entry is stale and goes, so the next
            // press reaches the one before it.
            const QStringList parts = text.split(QLatin1Char('\t'));
            QString why = parts.value(0);
            if (!parts.value(1).isEmpty()) {
                const QUrl u = urlOfKey(parts.value(1));
                why += QStringLiteral(": ") + rustDisplayName((u.isLocalFile() ? u.toLocalFile() : u.toDisplayString()).toUtf8());
            }
            telamon_hist_drop(self->m_engine, uint32_t(side));
            self->m_undoableId = 0;
            self->refreshHistory();
            Q_EMIT self->message(tr("Can't %1 \"%2\": %3").arg(verb, title, why));
            next();
            return;
        }

        // The steps: one job for all trashed copies (or all restores), one
        // per move, rmdir or mkdir.
        Work w;
        w.side = side;
        w.record = false;
        w.title = title;
        QList<QUrl> trashList, restoreList;
        for (const QString &line : text.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
            const QStringList f = line.split(QLatin1Char('\t'));
            const QString what = f.value(0);
            const QUrl path = urlOfKey(f.value(1));
            const QUrl to = urlOfKey(f.value(2));
            if (what == QLatin1String("trash_copy")) {
                trashList << path;
                w.top.insert(key(path));
            } else if (what == QLatin1String("restore")) {
                restoreList << path;
            } else if (what == QLatin1String("move_back")) {
                w.kind = Move;
                w.top.insert(key(path));
                w.steps << [path, to]() -> KJob * { return KIO::moveAs(path, to, KIO::HideProgressInfo); };
            } else if (what == QLatin1String("remove_folder")) {
                w.kind = NewFolder;
                w.steps << [path]() -> KJob * { return KIO::rmdir(path); };
            } else if (what == QLatin1String("make_folder")) {
                w.kind = NewFolder;
                w.steps << [path]() -> KJob * { return KIO::mkdir(path); };
            }
        }
        if (!trashList.isEmpty()) {
            w.kind = Trash;
            w.steps << [trashList]() -> KJob * { return KIO::trash(trashList, KIO::HideProgressInfo); };
        }
        if (!restoreList.isEmpty()) {
            w.kind = Restore;
            w.steps << [restoreList]() -> KJob * { return KIO::restoreFromTrash(restoreList, KIO::HideProgressInfo); };
        }
        if (w.steps.isEmpty()) {
            Q_EMIT self->message(tr("There is nothing to %1.").arg(verb));
            next();
            return;
        }
        w.histDone = next;
        const QString label = side == 0 ? tr("Undoing: %1").arg(title) : tr("Redoing: %1").arg(title);
        if (self->enqueue(std::move(w), label) == 0) {
            next();
        }
    });
}

// The undo or redo ran: the entry goes to the other side, as its inverse.
void OperationQueue::completeHistory(const Work &w)
{
    const int side = w.side;
    const QString title = w.title;
    QString trashes;
    for (auto it = w.pairs.begin(); it != w.pairs.end(); ++it) {
        trashes += it.key() + QLatin1Char('\t') + urlKey(it.value()) + QLatin1Char('\n');
    }
    const QString paths = OpsBridge::textOf([&](uint8_t *o, size_t c) { return telamon_hist_paths(m_engine, uint32_t(side), true, o, c); });
    QList<QUrl> urls;
    for (const QString &p : paths.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        urls << urlOfKey(p);
    }
    QPointer<OperationQueue> self(this);
    auto next = w.histDone;
    StatBatch::run(urls, this, [self, side, title, trashes, next](const SeenMap &seen) {
        if (!self) {
            return;
        }
        QString states;
        for (auto it = seen.begin(); it != seen.end(); ++it) {
            states += stateLine(it.key(), it.value());
        }
        const QByteArray st = states.toUtf8(), tr8 = trashes.toUtf8();
        // Once: it moves the entry, so it must not be repeated with a bigger buffer.
        telamon_hist_complete(self->m_engine, uint32_t(side), OpsBridge::p(st), OpsBridge::n(st), OpsBridge::p(tr8), OpsBridge::n(tr8), nullptr, 0);
        self->m_undoableId = 0;
        self->refreshHistory();
        Q_EMIT self->message(side == 0 ? tr("Undid: %1").arg(title) : tr("Redid: %1").arg(title));
        if (next) {
            next();
        }
    });
}

// A finished operation that can be undone: look at what it left, then record.
void OperationQueue::recordHistory(const Work &w)
{
    const quint64 id = w.id;
    const QString title = w.title;
    const Kind kind = w.kind;
    QList<QPair<QUrl, QUrl>> pairs;
    for (const QUrl &s : w.sources) {
        const auto it = w.pairs.constFind(key(s));
        if (it != w.pairs.constEnd()) {
            pairs.append({s, it.value()});
        }
    }
    const QList<QUrl> created = w.created;
    QList<QUrl> look;
    if (kind == Copy || kind == Link || kind == Move || kind == Rename || kind == Trash) {
        if (pairs.isEmpty()) {
            return;
        }
        for (const auto &p : std::as_const(pairs)) {
            look << p.second;
        }
    } else if (kind == NewFolder) {
        if (created.isEmpty()) {
            return;
        }
    } else {
        return;
    }
    QPointer<OperationQueue> self(this);
    const quint64 opId = id;
    histEnqueue([self, look, kind, title, pairs, created, opId](std::function<void()> next) {
        if (!self) {
            return;
        }
        StatBatch::run(look, self.data(), [self, kind, title, pairs, created, opId, next](const SeenMap &seen) {
            if (!self) {
                return;
            }
            const std::shared_ptr<void> done(nullptr, [next](void *) { next(); });
            QString rec;
            auto state = [&](const QUrl &u) { return seen.value(urlKey(u)); };
            switch (kind) {
            case NewFolder:
                rec = QStringLiteral("newfolder\n") + urlKey(created.first()) + QLatin1Char('\n');
                break;
            case Copy:
            case Link:
                rec = QStringLiteral("copy\n");
                for (const auto &p : pairs) {
                    if (!state(p.second).exists) {
                        return;
                    }
                    rec += urlKey(p.second) + QLatin1Char('\t') + stateFields(state(p.second)) + QLatin1Char('\n');
                }
                break;
            case Move:
            case Rename:
                rec = QStringLiteral("move\n");
                for (const auto &p : pairs) {
                    if (!state(p.second).exists) {
                        return;
                    }
                    rec += urlKey(p.first) + QLatin1Char('\t') + urlKey(p.second) + QLatin1Char('\t') + stateFields(state(p.second)) + QLatin1Char('\n');
                }
                break;
            case Trash:
                rec = QStringLiteral("trash\n");
                for (const auto &p : pairs) {
                    SeenState s = state(p.second);
                    if (!s.exists) {
                        // The Trash didn't answer: the file is known to be gone from
                        // where it was, which is all a restore needs to go by.
                        return;
                    }
                    rec += urlKey(p.first) + QLatin1Char('\t') + urlKey(p.second) + QLatin1Char('\t') + stateFields(s) + QLatin1Char('\n');
                }
                break;
            default:
                return;
            }
            const QByteArray t = title.toUtf8(), r = rec.toUtf8();
            if (telamon_hist_record(self->m_engine, OpsBridge::p(t), OpsBridge::n(t), OpsBridge::p(r), OpsBridge::n(r))) {
                self->m_undoableId = opId;
                self->refreshHistory();
                self->refresh();
            }
        });
    });
}
