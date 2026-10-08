// What Files does itself when it takes things out of an archive through KIO
// (a drag out of `zip:/`, a paste, the Extract button without Telamon
// Archive): look at what the archive lists first, and refuse the whole job if
// any entry would land outside the folder chosen (`..`, an absolute path, a
// link that points out; the core's `archive` module has the rules). KIO's copy
// then does the work through the operation queue, so a name that is taken
// still opens the conflict dialog. docs/DESIGN.md, "Archives".
#pragma once

#include <KJob>

#include <QByteArray>
#include <QList>
#include <QPointer>
#include <QUrl>

#include <functional>
#include <memory>

namespace ArchiveCheck
{
// Looks, on a worker, at the zip files among `files` (local files) for the
// flag that says their entries are encrypted: KIO's archive worker can't
// decrypt and would hand out the encrypted bytes as if they were the files.
// `done` gets the name of the first that needs a password (`unknown` false),
// or the first whose directory can't be read well enough to tell (`unknown`
// true), or an empty name.
void findEncryptedZip(const QList<QUrl> &files, QObject *context, std::function<void(const QString &name, bool unknown)> done);
// The words for them; `archiveInstalled` adds where a password can be given.
QString needsPasswordText(const QString &name, bool archiveInstalled);
QString undecidedText(const QString &name, bool archiveInstalled);
}

// What the steps of one extraction hand to each other.
struct ArchivePlan {
    // The entries at the top of an archive, to be copied into `dest`.
    QList<QUrl> children;
    // Bytes the entries hold once taken out.
    quint64 bytes = 0;
    // The folder they go into.
    QUrl dest;
    // What to select when it is done.
    QList<QUrl> results;
};

// Lists `sources` (entries of archives; a folder is looked into) or, with
// `root`, the top of one archive, and refuses what can't be taken out safely.
class ArchiveGuardJob : public KJob
{
    Q_OBJECT

public:
    // The error code of a refusal (the window shows its text in a dialog).
    static constexpr int Refused = KJob::UserDefinedError + 1001;
    // An encrypted zip: Files can't enter the password.
    static constexpr int NeedsPassword = KJob::UserDefinedError + 1003;

    ArchiveGuardJob(const QList<QUrl> &sources, bool root, std::shared_ptr<ArchivePlan> plan, bool archiveInstalled, QObject *parent = nullptr);

    void start() override;

protected:
    bool doKill() override;

private:
    void begin();
    void next();
    void listed(const QUrl &src, const QString &top, bool root);
    void finishChecks();
    void fail(int code, const QString &text);
    void addRecord(const QString &path, const QString &link, bool isLink, quint64 size);

    QList<QUrl> m_sources;
    bool m_root;
    std::shared_ptr<ArchivePlan> m_plan;
    bool m_archiveInstalled;
    int m_at = 0;
    QByteArray m_records;
    qint64 m_count = 0;
    quint64 m_bytes = 0;
    QPointer<KJob> m_sub;
    bool m_done = false;
    bool m_toobig = false;
};

// Picks a free folder name in `parent` (a folder on this computer) when
// asked, then checks there is room for `plan->bytes` there; on a worker. Sets
// `plan->dest`.
class ArchivePrepareJob : public KJob
{
    Q_OBJECT

public:
    static constexpr int NoRoom = KJob::UserDefinedError + 1002;

    // `name` empty: `parent` is the folder, nothing is picked.
    ArchivePrepareJob(const QUrl &parent, const QString &name, const QString &what, std::shared_ptr<ArchivePlan> plan, QObject *parent_ = nullptr);

    void start() override;

protected:
    bool doKill() override { return true; }

private:
    QUrl m_parent;
    QString m_name;
    QString m_what;
    std::shared_ptr<ArchivePlan> m_plan;
};

// A step that has nothing to do (an empty archive).
class NoopJob : public KJob
{
    Q_OBJECT

public:
    explicit NoopJob(QObject *parent = nullptr);
    void start() override {}

protected:
    bool doKill() override { return true; }
};
