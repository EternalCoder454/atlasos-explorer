// Telamon Archive's Archive1 D-Bus interface, as Files calls it (docs/DESIGN.md,
// "Archive1, as Explorer uses it"). Files never reads an archive's bytes for
// this: Archive extracts and compresses, and the job it returns is shown and
// paced in Files' own operation queue (`ArchiveJob` is a KJob, so the queue
// treats it like any other job: progress, Pause, Resume, Cancel).
#pragma once

#include <KJob>

#include <QDBusMessage>
#include <QList>
#include <QPointer>
#include <QString>
#include <QTimer>
#include <QUrl>
#include <QVariantMap>

#include <functional>
#include <memory>

namespace ArchiveBus
{
// One name Archive answers on: the Telamon one first, then the old Atlas one,
// which Archive keeps for one release.
struct Target {
    QString service;
    QString path;
    QString iface;
    QString jobIface() const { return iface + QStringLiteral(".Job"); }
};
QList<Target> targets();

// What went wrong with a call, in plain words (`busy`: Archive has 16 jobs
// waiting; `notRunning`: no owner of any of its names).
struct Failure {
    QString text;
    bool busy = false;
    bool notRunning = false;
};
Failure failureOf(const QString &errorName, const QString &message);
QString busyText();
QString notRunningText();

// Calls `method` on the first of Archive's names that has an owner, with a
// timeout of 10 s, and tells `done` how it went: the reply (empty on a
// failure) and what failed. `context` ends the wait for an answer when it is
// deleted (the call itself is not taken back).
void call(QObject *context, const QString &method, const QVariantList &args, std::function<void(const QDBusMessage &reply, const Target &target, const Failure &failure)> done);
}

// A job Archive runs: starts it with `method` (ExtractHere, ExtractEntries,
// Compress), follows it through its Job object and ends when Archive says it
// is done. `results()` are the files it made, for the window to select.
class ArchiveJob : public KJob
{
    Q_OBJECT

public:
    // `args` come before the options, as the method takes them.
    ArchiveJob(const QString &method, const QVariantList &args, const QVariantMap &options, QObject *parent = nullptr);
    ~ArchiveJob() override;

    void start() override;
    QList<QUrl> results() const { return m_results; }

Q_SIGNALS:
    // Archive is asking the user something (a password, a name that is taken)
    // in its own window.
    void needsUser();

protected:
    bool doKill() override;
    bool doSuspend() override;
    bool doResume() override;

private Q_SLOTS:
    void onFinishedMessage(const QDBusMessage &message);
    void onPropertiesChanged(const QString &iface, const QVariantMap &changed, const QStringList &invalidated);

private:
    struct Shared {
        // What Cancel and Pause must reach if they are asked before the job
        // has a path.
        ArchiveBus::Target target;
        QString path;
        bool cancel = false;
        bool pause = false;
    };

    void begin();
    void attachTo(const QString &path);
    void readProperties(const QVariantMap &props);
    void finish(const QString &state, const QStringList &results);
    void failWith(const QString &text);
    static void send(const ArchiveBus::Target &target, const QString &path, const QString &method);

    QString m_method;
    QVariantList m_args;
    QVariantMap m_options;
    std::shared_ptr<Shared> m_shared = std::make_shared<Shared>();
    QList<QUrl> m_results;
    QString m_state;
    QString m_error;
    bool m_finished = false;
    // The result was given (or the job was killed): nothing more is said.
    bool m_reported = false;
    bool m_asked = false;
    // A Finished that arrived before the call had returned its path.
    QList<QDBusMessage> m_early;
    QTimer m_lateFinish;
};
